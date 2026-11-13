//! Cardinal bottom pages, adapted without changing the approved gem renderer.
//! Geometry is from DawMixerLayout.h/DawFaderSystem.cpp. This module supplies
//! control poses, labels, shared picking, and typed actions to layer five.
//! It never opens a device/plugin or changes a document behind its owner's back.
use super::{
    loopback_history::{Copy as AudioCopy, History, Source},
    spectral_repair::{Mask, norm_to_hz},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Mixer,
    Plugins,
    Loopback,
    ClipEditor,
}
impl Page {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Mixer => 0,
            Self::Plugins => 1,
            Self::Loopback => 2,
            Self::ClipEditor => 3,
        }
    }
    pub(crate) fn from_index(index: usize) -> Self {
        [Self::Mixer, Self::Plugins, Self::Loopback, Self::ClipEditor][index.min(3)]
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorTab {
    #[default]
    Waveform,
    Warp,
    Spectral,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}
impl Rect {
    pub(crate) fn contains(self, p: [f64; 2]) -> bool {
        self.w > 0.
            && self.h > 0.
            && p.iter().all(|v| v.is_finite())
            && p[0] >= self.x
            && p[0] < self.x + self.w
            && p[1] >= self.y
            && p[1] < self.y + self.h
    }
    fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            w: (self.x + self.w).min(other.x + other.w).max(x) - x,
            h: (self.y + self.h).min(other.y + other.h).max(y) - y,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ControlId {
    SelectPage(usize),
    SelectPluginTrack(usize),
    NextSoundtrack,
    NextSpeaker,
    LoopbackTab,
    RecallAudio,
    RecallMidi,
    Memory,
    Source,
    ClearBuffer,
    Waveform,
    Warp,
    Spectral,
    Strength,
    Attenuate,
    UndoMask,
    ClearMasks,
    Feather,
    WarpToggle,
    ClipOutput,
    ClipGainDown, ClipGainReset, ClipGainUp, WaveTrim, WaveSplit,
    WarpPitchDown, WarpPitchUp, WarpAdd, WarpRemove,
    SelectTrack(String),
    SelectBus(String),
    Fader(String),
}
#[derive(Clone, Debug)]
pub(crate) struct Control {
    pub id: ControlId,
    pub label: String,
    pub rect: Rect,
    pub active: bool,
    pub enabled: bool,
    pub pressure: f64,
}
#[derive(Clone, Debug)]
pub(crate) struct Track {
    pub id: String,
    pub label: String,
    pub gain: f64,
    pub meter: [f64; 2],
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AudioClip {
    pub trackless:bool,
    pub output:String,
    pub id: String,
    pub source_offset: u64,
    pub frames: u64,
    pub sample_rate: u32,
    pub gain:f64,
    pub start_seconds:f64,
    pub warp:serde_json::Value,
    pub warp_enabled: bool,
    pub spectral_masks: usize,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Context {
    pub revision: u64,
    pub tracks: Vec<Track>,
    pub selected_audio: Option<AudioClip>,
    pub plugin_track_count: usize,
    pub plugin_track_index: usize,
    pub buses:Vec<serde_json::Value>,
    pub selected_bus:Option<String>,
    pub master_meters: [f64; 6],
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Input {
    pub pointer: Option<[f64; 2]>,
    pub left: bool,
    pub wheel: f64,
    pub command: bool,
    pub focused: bool,
    pub cancelled: bool,
}
#[derive(Debug)]
pub(crate) enum Action {
    SelectTrack(String),
    SelectBus(String),
    PageChanged(Page),
    SelectPluginTrack(usize),
    NextSoundtrack,
    NextSpeaker,
    SetTrackGain {
        revision: u64,
        id: String,
        gain: f64,
    },
    RecallAudio {
        seconds: u32,
    },
    RecallMidi {
        seconds: u32,
    },
    SetCaptureSource(Source),
    EditorLoop {start:f64,end:f64},
    ClearCapture,
    CopyLoopback {
        audio: AudioCopy,
        pointer: [f64; 2],
        snap: bool,
    },
    ClipEdit {revision:u64,id:String,hook:&'static str,input:serde_json::Value},
    SetClipWarp {
        revision: u64,
        id: String,
        enabled: bool,
    },
    AddSpectralMask {
        revision: u64,
        id: String,
        mask: Mask,
    },
    UndoSpectralMask {
        revision: u64,
        id: String,
    },
    ClearSpectralMasks {
        revision: u64,
        id: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpectralSelection {
    pub x0: f64,
    pub x1: f64,
    pub f0: f64,
    pub f1: f64,
}
#[derive(Clone, Debug)]
pub(crate) struct Fader {
    pub track: String,
    pub housing: Rect,
    pub knob: Rect,
    pub normalized: f64,
    pub meters: [Rect; 2],
    pub levels: [f64; 2],
}
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    pub panel: Rect,
    pub content: Rect,
    pub controls: Vec<Control>,
    pub faders: Vec<Fader>,
    pub view: Rect,
    pub master_meters: Vec<(String, Rect, f64)>,
}
impl Layout {
    pub(crate) fn hit(&self, p: [f64; 2]) -> Option<&Control> {
        if !self.panel.contains(p) {
            return None;
        }
        self.controls
            .iter()
            .find(|c| c.enabled && c.rect.intersect(self.panel).contains(p))
    }
}
#[derive(Clone, Debug)]
enum Gesture {
    Button(ControlId),
    Fader { id: String, offset: f64 },
    Spectral { clip: AudioClip },
    LoopSelection { anchor: u64 },
    LoopCopy,
    EditorBar {zoom:bool,anchor:f64,clip:AudioClip,revision:u64,panel:Rect},
    Wave {clip:AudioClip,revision:u64,anchor:f64,panel:Rect},
    WarpMarker {clip:AudioClip,revision:u64,index:usize,markers:serde_json::Value,panel:Rect},
}
#[derive(Debug)]
pub(crate) struct State {
    pub page: Page,
    pub editor_tab: EditorTab,
    pub mixer_scroll: f64,
    plugin_nav_start: usize,
    plugin_nav_selected: usize,
    pub attenuation_db: f32,
    pub feather: f32,
    pub spectral_selection: Option<SpectralSelection>,
    pub wave_selection:Option<[f64;2]>,
    pub wave_view:[f64;2],
    pub wave_loop:Option<[f64;2]>,
    pub warp_selected:Option<usize>,
    pub loopback: History,
    pressure: BTreeMap<ControlId, f64>,
    gesture: Option<Gesture>,
    left_held: bool,
    blocked_until_release: bool,
    cancelled_capture: bool,
    selected_audio: Option<AudioClip>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            page: Page::Mixer,
            editor_tab: EditorTab::Waveform,
            mixer_scroll: 0.,
            plugin_nav_start: 0,
            plugin_nav_selected: 0,
            attenuation_db: -24.,
            feather: 0.12,
            spectral_selection: None,
            wave_selection:None,warp_selected:None,wave_view:[0.,1.],wave_loop:None,
            loopback: History::default(),
            pressure: BTreeMap::new(),
            gesture: None,
            left_held: false,
            blocked_until_release: false,
            cancelled_capture: false,
            selected_audio: None,
        }
    }
}
impl State {
    /// Audio context-menu action: select Waveform and expand/open the
    /// bottom panel through its motion owner after this method. No new button.
    pub(crate) fn open_clip_editor(&mut self, left: bool) {
        self.set_page(Page::ClipEditor, left);
        self.editor_tab = EditorTab::Waveform;
        self.spectral_selection = None;
    }
    /// Original physical depression: 0..0.5 over 150 ms; renderer applies this
    /// to the authored model, never by tinting a cached screen rectangle.
    pub(crate) fn advance_pressure(&mut self, dt: f64) {
        if !dt.is_finite() {
            return;
        }
        let active = match &self.gesture {
            Some(Gesture::Button(id)) => Some(id.clone()),
            Some(Gesture::Fader { id, .. }) => Some(ControlId::Fader(id.clone())),
            _ => None,
        };
        if let Some(id) = &active {
            self.pressure.entry(id.clone()).or_insert(0.);
        }
        for (id, value) in &mut self.pressure {
            let target = if Some(id) == active.as_ref() && self.left_held {
                0.5
            } else {
                0.
            };
            let step = dt.max(0.) * (0.5 / 0.15);
            *value = if *value < target {
                (*value + step).min(target)
            } else {
                (*value - step).max(target)
            };
        }
        self.pressure.retain(|_, value| *value > 0.);
    }
    pub(crate) fn warp_preview(&self)->Option<(&str,&serde_json::Value)>{match &self.gesture{Some(Gesture::WarpMarker{clip,markers,..})=>Some((&clip.id,markers)),_=>None}}
    pub(crate) fn dragging_fader(&self)->bool {matches!(self.gesture,Some(Gesture::Fader{..}))}
    pub(crate) fn consumes_pointer(&self) -> bool {
        self.gesture.is_some() || self.cancelled_capture
    }
    pub(crate) fn cancel(&mut self, left: bool) {
        // Remember held edges without claiming another panel's gesture.
        self.cancelled_capture=left&&(self.gesture.is_some()||self.cancelled_capture);
        if matches!(self.gesture,Some(Gesture::Wave{..})){self.wave_selection=None;}
        self.gesture = None;
        self.left_held = left;
        self.blocked_until_release = left;
    }
    pub(crate) fn set_page(&mut self, page: Page, left: bool) {
        if self.page != page {
            self.page = page;
            self.cancel(left);
        }
    }
    pub(crate) fn sync(&mut self, context: &Context) {
        if self.plugin_nav_selected != context.plugin_track_index {
            self.plugin_nav_selected = context.plugin_track_index;
            if context.plugin_track_index < self.plugin_nav_start || context.plugin_track_index >= self.plugin_nav_start + 3 {
                self.plugin_nav_start = context.plugin_track_index / 3 * 3;
            }
        }
        self.plugin_nav_start = self.plugin_nav_start.min(context.plugin_track_count.saturating_sub(1) / 3 * 3);
        // A stale spectral selection or gesture must never be applied to the
        // next selected clip, or to the old source range after an external edit.
        if self.selected_audio != context.selected_audio {
            let same_range = match (&self.selected_audio, &context.selected_audio) {
                (Some(a), Some(b)) => {
                    a.id == b.id
                        && a.source_offset == b.source_offset
                        && a.frames == b.frames
                        && a.sample_rate == b.sample_rate
                }
                _ => false,
            };
            if !same_range {
                self.spectral_selection = None;
                self.wave_selection=None;self.warp_selected=None;self.wave_view=[0.,1.];self.wave_loop=None;
                if matches!(self.gesture, Some(Gesture::Spectral { .. })) {
                    self.cancelled_capture=self.left_held;
                    self.gesture = None;
                    self.blocked_until_release = self.left_held;
                }
            }
            self.selected_audio = context.selected_audio.clone();
        }
        if matches!(&self.gesture,Some(Gesture::Fader{id,..}) if !context.tracks.iter().any(|t|&t.id==id))
        {
            self.cancelled_capture=self.left_held;
            self.gesture = None;
            self.blocked_until_release = self.left_held;
        }
    }
    /// Use the actual logical bottom panel rectangle, including swapping,
    /// expansion and origami transforms already resolved by panel_layout.
    pub(crate) fn layout(&self, panel: Rect, context: &Context) -> Layout {
        let x = panel.x + 40.;
        let y = panel.y + 36.;
        let left = x + 144.;
        let right = panel.x + panel.w - 40.;
        let bottom = panel.y + panel.h - 24.;
        let content = Rect {
            x: left,
            y,
            w: (right - left).max(0.),
            h: (bottom - y).max(0.),
        };
        let mut result = Layout {
            panel,
            content,
            controls: Vec::new(),
            faders: Vec::new(),
            view: Rect::default(),
            master_meters: Vec::new(),
        };
        let mut add = |id, label: &str, rect, active, enabled| {
            result.controls.push(Control {
                pressure: self.pressure.get(&id).copied().unwrap_or(0.),
                id,
                label: label.into(),
                rect,
                active,
                enabled,
            })
        };
        // Match the right-panel page gems: 43.2-point faces on 48-point centers.
        // Buffer retains its internal page ID but is deliberately not exposed.
        for (number, page) in [Page::Mixer, Page::Plugins, Page::ClipEditor].into_iter().enumerate() {
            add(ControlId::SelectPage(page.index()), &(number + 1).to_string(),
                Rect { x: x + number as f64 * 48., y, w: 43.2, h: 43.2 },
                self.page == page, true);
        }
        match self.page {
            Page::Plugins => {
                // Match the page gems above; wheel scrolls additional groups
                // without shrinking gems or exceeding the bounded GPU packet.
                for index in self.plugin_nav_start..(self.plugin_nav_start + 3).min(context.plugin_track_count) {
                    add(ControlId::SelectPluginTrack(index), &(index + 1).to_string(),
                        Rect { x: x + (index - self.plugin_nav_start) as f64 * 48., y: y + 54., w: 43.2, h: 43.2 },
                        context.plugin_track_index == index, true);
                }
            }
            Page::Mixer => {
                // Rectangles describe the 156 mm spectrum, not the complete
                // 180 mm housing. Reserve its 12 mm end rims inside the clip,
                // with one logical pixel of clearance at each outer edge.
                let meter_housing_height = (content.h - 22. - 2.).max(0.);
                let meter_y = y + 1. + meter_housing_height * (12. / 180.);
                let meter_height = meter_housing_height * (156. / 180.);
                for (i, track) in context.tracks.iter().enumerate() {
                    let column = left + self.mixer_scroll + i as f64 * 90.;
                    let normalized = gain_to_fader(track.gain);
                    // Authored slider travel inside the 180 mm casing, rather
                    // than a separately inset screen-space rail.
                    let (top, usable) = fader_travel(y, content.h);
                    let center_y = top + (1. - normalized) * usable;
                    let knob = Rect {
                        x: column + 27.5,
                        y: center_y - 12.5,
                        w: 25.,
                        h: 25.,
                    };
                    let meters = [
                        Rect {
                            x: column,
                            y: meter_y,
                            w: 5.,
                            h: meter_height,
                        },
                        Rect {
                            x: column + 5.,
                            y: meter_y,
                            w: 5.,
                            h: meter_height,
                        },
                    ];
                    add(ControlId::SelectTrack(track.id.clone()), &track.label,
                        Rect{x:column-4.,y:y-20.,w:84.,h:20.},false,true);
                    result.faders.push(Fader {
                        track: track.id.clone(),
                        housing: Rect {
                            x: column + 20.,
                            y,
                            w: 40.,
                            h: (content.h - 22.).max(0.),
                        },
                        knob,
                        normalized,
                        meters,
                        levels: track.meter,
                    });
                    add(
                        ControlId::Fader(track.id.clone()),
                        &track.label,
                        knob,
                        false,
                        content.w > 1. && content.h > 71.,
                    );
                    add(ControlId::SelectTrack(track.id.clone()), &track.label,
                        Rect{x:column+20.,y,w:40.,h:(content.h-22.).max(0.)},false,true);
                }
                let master_start =
                    left + self.mixer_scroll + context.tracks.len() as f64 * 90. + 30.;
                // Cardinal's 25-point pitch described bare 10-point spectra.
                // Our 30-point gem casings need a gap of their own. Reserve
                // the same 22-point readout band as the adjacent fader housing.
                let master_spacing = 40.;
                let master_housing_height = (content.h - 22. - 2.).max(0.);
                let master_y = y + 1. + master_housing_height * (12. / 180.);
                let master_height = master_housing_height * (156. / 180.);
                let mut column=0;
                for bus in &context.buses {
                    let id=bus["id"].as_str().unwrap_or("");
                    let channels=if bus["kind"]!="matrix"{2}else{1};
                    for ch in 0..channels {
                        let r=Rect{x:master_start+column as f64*master_spacing,y:master_y,w:10.,h:master_height};
                        let level=bus["meter"][ch].as_f64().unwrap_or(0.);
                        let name=bus["name"].as_str().unwrap_or(id);
                        let label=if bus["kind"]=="channel"{format!("CH{}",if ch==0{"L"}else{"R"})}else if channels==2{format!("MX{}{}",context.buses.iter().filter(|b|b["kind"]=="mix").position(|b|b["id"]==id).unwrap_or(0)+1,if ch==0{"L"}else{"R"})}else{format!("M{}",bus["input"].as_str().unwrap_or("?"))};
                        let _=name;
                        result.master_meters.push((label.clone(),r,level));
                        add(ControlId::SelectBus(id.into()),&label,Rect{x:r.x-10.,y:y+1.,w:30.,h:master_housing_height},context.selected_bus.as_deref()==Some(id),true);
                        column+=1;
                    }
                    column+=1;
                }
            }
            Page::Loopback => {
                let controls_y = y + 52.;
                add(
                    ControlId::LoopbackTab,
                    self.loopback.source().name(),
                    Rect {
                        x: left,
                        y,
                        w: 96.,
                        h: 30.,
                    },
                    true,
                    true,
                );
                add(
                    ControlId::RecallAudio,
                    "RECALL AUDIO",
                    Rect {
                        x: left + 102.,
                        y,
                        w: 112.,
                        h: 30.,
                    },
                    false,
                    true,
                );
                add(
                    ControlId::RecallMidi,
                    "RECALL MIDI",
                    Rect {
                        x: left + 220.,
                        y,
                        w: 112.,
                        h: 30.,
                    },
                    false,
                    true,
                );
                add(
                    ControlId::Memory,
                    &format!("MEM {} SEC", self.loopback.memory_seconds()),
                    Rect {
                        x: left,
                        y: controls_y,
                        w: 112.,
                        h: 28.,
                    },
                    false,
                    true,
                );
                add(
                    ControlId::Source,
                    self.loopback.source().label(),
                    Rect {
                        x: left + 120.,
                        y: controls_y,
                        w: 138.,
                        h: 28.,
                    },
                    self.loopback.source() == Source::Microphone,
                    true,
                );
                add(
                    ControlId::ClearBuffer,
                    "CLEAR",
                    Rect {
                        x: right - 76.,
                        y: controls_y,
                        w: 76.,
                        h: 28.,
                    },
                    false,
                    true,
                );
                let wave_y = controls_y + 38.;
                result.view = Rect {
                    x: left + 8.,
                    y: wave_y + 24.,
                    w: (right - left - 16.).max(0.),
                    h: (bottom - wave_y - 32.).max(0.),
                };
            }
            Page::ClipEditor => {
                let valid = context.selected_audio.is_some();
                let controls_y = y + 52.;
                for (i, (id, tab, label)) in [
                    (ControlId::Waveform, EditorTab::Waveform, "WAVEFORM"),
                    (ControlId::Warp, EditorTab::Warp, "WARP"),
                    (ControlId::Spectral, EditorTab::Spectral, "SPECTRAL"),
                ]
                .into_iter()
                .enumerate()
                {
                    add(
                        id,
                        label,
                        Rect {
                            x: left + i as f64 * 106.,
                            y,
                            w: 100.,
                            h: 30.,
                        },
                        self.editor_tab == tab,
                        tab != EditorTab::Spectral,
                    );
                }
                if valid && self.editor_tab == EditorTab::Waveform {
                    if let Some(c)=context.selected_audio.as_ref().filter(|c|c.trackless){
                        let label=context.buses.iter().find(|b|b["id"]==c.output).map(|b|if b["kind"]=="matrix"{format!("TO {}",b["input"].as_str().unwrap_or("?"))}else{format!("TO {}",b["name"].as_str().unwrap_or("CHANNEL"))}).unwrap_or("TO OFF".into());
                        add(ControlId::ClipOutput,&label.chars().take(16).collect::<String>(),Rect{x:left+446.,y:controls_y,w:124.,h:28.},false,true);
                    }
                    let gain=context.selected_audio.as_ref().map_or(1.,|c|c.gain);
                    for (id,label,offset,width,enabled) in [
                        (ControlId::ClipGainDown,"GAIN -1".into(),0.,82.,valid),
                        (ControlId::ClipGainReset,if gain>0.{format!("{:+.1} DB",20.*gain.log10())}else{"-INF DB".into()},88.,100.,valid),
                        (ControlId::ClipGainUp,"GAIN +1".into(),194.,82.,valid),
                        (ControlId::WaveTrim,"TRIM".into(),282.,76.,valid&&context.selected_audio.as_ref().is_some_and(|c|!c.warp_enabled)&&self.wave_selection.is_some_and(|s|s[0]!=s[1])),
                        (ControlId::WaveSplit,"SPLIT".into(),364.,76.,valid&&context.selected_audio.as_ref().is_some_and(|c|!c.warp_enabled)&&self.wave_selection.is_some_and(|s|s[0]>0.&&s[0]<1.))] {
                        add(id,&label,Rect{x:left+offset,y:controls_y,w:width,h:28.},false,enabled);
                    }
                }
                if valid && self.editor_tab == EditorTab::Warp {
                    let enabled = context
                        .selected_audio
                        .as_ref()
                        .is_some_and(|c| c.warp_enabled);
                    add(
                        ControlId::WarpToggle,
                        if enabled { "WARP ON" } else { "WARP OFF" },
                        Rect {
                            x: left,
                            y: controls_y,
                            w: 118.,
                            h: 28.,
                        },
                        enabled,
                        valid,
                    );
                }
                if valid && self.editor_tab == EditorTab::Warp {
                    for (id,label,offset,width) in [(ControlId::WarpPitchDown,"PITCH -1",124.,86.),(ControlId::WarpPitchUp,"PITCH +1",216.,86.),(ControlId::WarpAdd,"ADD MARK",308.,90.),(ControlId::WarpRemove,"REMOVE",404.,86.)] {
                        let enabled = context.selected_audio.as_ref().is_some_and(|c| {
                            if !c.warp_enabled { return false; }
                            match id {
                                ControlId::WarpAdd => self.wave_selection.is_some_and(|s| {
                                    let source = c.source_offset as f64 + s[0] * c.frames as f64;
                                    c.warp["markers"].as_array().is_some_and(|markers| markers.windows(2).any(|p| {
                                        source > p[0]["source_frame"].as_f64().unwrap_or(f64::INFINITY) + 1.
                                            && source < p[1]["source_frame"].as_f64().unwrap_or(f64::NEG_INFINITY) - 1.
                                    }))
                                }),
                                ControlId::WarpRemove => c.warp["markers"].as_array().is_some_and(|markers|
                                    self.warp_selected.is_some_and(|i| i > 0 && i + 1 < markers.len())),
                                _ => true,
                            }
                        });
                        add(id,label,Rect{x:left+offset,y:controls_y,w:width,h:28.},false,enabled);
                    }
                }
                if valid && self.editor_tab == EditorTab::Spectral {
                    for (id, offset, width, label, enabled) in [
                        (
                            ControlId::Strength,
                            0.,
                            105.,
                            format!("{} dB", self.attenuation_db.round()),
                            valid,
                        ),
                        (
                            ControlId::Attenuate,
                            113.,
                            112.,
                            "ATTENUATE".into(),
                            valid && self.spectral_selection.is_some(),
                        ),
                        (
                            ControlId::UndoMask,
                            233.,
                            90.,
                            "UNDO MASK".into(),
                            valid
                                && context
                                    .selected_audio
                                    .as_ref()
                                    .is_some_and(|c| c.spectral_masks > 0),
                        ),
                        (
                            ControlId::ClearMasks,
                            331.,
                            96.,
                            "CLEAR MASKS".into(),
                            valid
                                && context
                                    .selected_audio
                                    .as_ref()
                                    .is_some_and(|c| c.spectral_masks > 0),
                        ),
                        (
                            ControlId::Feather,
                            435.,
                            112.,
                            format!("FEATHER {}%", (self.feather * 100.).round()),
                            valid,
                        ),
                    ] {
                        add(
                            id,
                            &label,
                            Rect {
                                x: left + offset,
                                y: controls_y,
                                w: width,
                                h: 28.,
                            },
                            false,
                            enabled,
                        );
                    }
                }
                result.view = Rect {
                    x: left,
                    y: controls_y + if self.editor_tab==EditorTab::Waveform {82.}else{38.},
                    w: content.w,
                    h: (bottom - controls_y - if self.editor_tab==EditorTab::Waveform {82.}else{38.}).max(0.),
                };
            }
        }
        if self.page == Page::ClipEditor {
            // Match the gemstone's planar face (PanelLayout's 0.31 * minor
            // safe cut). The editor has no mixer/navigation gutter. Keep all
            // buttons in one row on the upper bevel, outside the display.
            let inset = panel.w.min(panel.h) * 0.155;
            let face = Rect {
                x: panel.x + inset,
                y: panel.y + inset,
                w: (panel.w - 2. * inset).max(0.),
                h: (panel.h - 2. * inset).max(0.),
            };
            let header_width = 144. + 318. + 547.;
            let scale = (face.w / header_width).min(1.);
            let button_height = 43.2f64.min(inset * 0.5);
            let button_y = panel.y + (inset - button_height) * 0.75;
            for control in &mut result.controls {
                let offset = if matches!(control.id, ControlId::SelectPage(_)) {
                    control.rect.x - x
                } else if control.rect.y == y {
                    144. + control.rect.x - left
                } else {
                    144. + 318. + control.rect.x - left
                };
                control.rect = Rect {
                    x: face.x + offset * scale,
                    y: button_y,
                    w: control.rect.w * scale,
                    h: button_height,
                };
            }
            result.content = face;
            // Waveform zoom/loop strips live at the top of the display face.
            let bars = if self.editor_tab == EditorTab::Waveform { 44f64.min(face.h) } else { 0. };
            result.view = Rect { y: face.y + bars, h: face.h - bars, ..face };
        }
        result
    }
    pub(crate) fn update(&mut self, input: &Input, panel: Rect, context: &Context) -> Vec<Action> {
        self.sync(context);
        let pressed = input.left && !self.left_held;
        let released = !input.left && self.left_held;
        self.left_held = input.left;
        if !input.focused
            || input.cancelled
            || ![panel.x, panel.y, panel.w, panel.h]
                .iter()
                .all(|v| v.is_finite())
            || panel.w <= 1.
            || panel.h <= 1.
        {
            self.cancel(input.left);
            return Vec::new();
        }
        if self.blocked_until_release {
            self.blocked_until_release = input.left;
            self.cancelled_capture &= input.left;
            return Vec::new();
        }
        let mut result = Vec::new();
        let layout = self.layout(panel, context);
        if self.page == Page::Plugins && input.wheel.is_finite() && input.wheel != 0. && self.gesture.is_none()
            && input.pointer.is_some_and(|p| Rect { x: panel.x + 40., y: panel.y + 90., w: 144., h: 43.2 }.contains(p)) {
            let last = context.plugin_track_count.saturating_sub(1) / 3 * 3;
            self.plugin_nav_start = if input.wheel > 0. { (self.plugin_nav_start + 3).min(last) } else { self.plugin_nav_start.saturating_sub(3) };
        }
        if self.page==Page::ClipEditor && self.editor_tab==EditorTab::Waveform
            && input.wheel.is_finite() && input.wheel!=0. && self.gesture.is_none()
            && input.pointer.is_some_and(|p|layout.view.contains(p)) {
            if let Some(clip)=&context.selected_audio {
                let width=self.wave_view[1]-self.wave_view[0];
                let x=wave_point(layout.view,input.pointer.unwrap());
                let next=if input.command {(width*1.18f64.powf(input.wheel.clamp(-100.,100.))).clamp(1./clip.frames.max(1) as f64,1.)}else{width};
                let start=if input.command {self.wave_view[0]+x*(width-next)}else{self.wave_view[0]+input.wheel*width*0.1};
                let start=start.clamp(0.,1.-next);
                self.wave_view=[start,start+next];
            }
        }
        if self.page == Page::Mixer
            && input.wheel.is_finite()
            && input.wheel != 0.
            && input.pointer.is_some_and(|p| panel.contains(p))
            && self.gesture.is_none()
        {
            let bus_width=context.buses.iter().map(|b|if b["kind"]!="matrix"{120.}else{80.}).sum::<f64>();
            let extra = layout.content.w - context.tracks.len() as f64 * 90. - bus_width - 60.;
            let min = if extra >= 0. { -extra } else { extra };
            let max = extra.max(0.);
            self.mixer_scroll = (self.mixer_scroll + input.wheel * 40.).clamp(min, max);
        }
        if pressed {
            if let Some(pointer) = input.pointer {
                if self.page==Page::ClipEditor && self.editor_tab==EditorTab::Waveform && pointer[0]>=layout.view.x && pointer[0]<layout.view.x+layout.view.w && pointer[1]>=layout.view.y-44. && pointer[1]<layout.view.y-4. {
                    if let Some(clip)=context.selected_audio.clone(){let zoom=pointer[1]<layout.view.y-24.;let x=((pointer[0]-layout.view.x)/layout.view.w).clamp(0.,1.);let anchor=if zoom{x}else{self.wave_view[0]+x*(self.wave_view[1]-self.wave_view[0])};self.gesture=Some(Gesture::EditorBar{zoom,anchor,clip,revision:context.revision,panel});}
                } else if let Some(control) = layout.hit(pointer) {
                    self.gesture = Some(match &control.id {
                        ControlId::Fader(id) => { result.push(Action::SelectTrack(id.clone())); Gesture::Fader {
                            id: id.clone(),
                            offset: control.rect.y + control.rect.h * 0.5 - pointer[1],
                        }},
                        _ => Gesture::Button(control.id.clone()),
                    });
                } else if layout.view.contains(pointer) && panel.contains(pointer) {
                    if self.page == Page::Loopback {
                        if let Some(frame) = self
                            .loopback
                            .frame_at_fraction((pointer[0] - layout.view.x) / layout.view.w)
                        {
                            if self
                                .loopback
                                .selection()
                                .is_some_and(|s| frame >= s.start && frame < s.end)
                            {
                                self.gesture = Some(Gesture::LoopCopy);
                            } else {
                                self.loopback.select(frame, frame);
                                self.gesture = Some(Gesture::LoopSelection { anchor: frame });
                            }
                        }
                    } else if self.page==Page::ClipEditor && self.editor_tab==EditorTab::Waveform {
                        if let Some(clip)=&context.selected_audio {let x=self.wave_view[0]+wave_point(layout.view,pointer)*(self.wave_view[1]-self.wave_view[0]);self.wave_selection=Some([x,x]);self.gesture=Some(Gesture::Wave{clip:clip.clone(),revision:context.revision,anchor:x,panel});}
                    } else if self.page==Page::ClipEditor && self.editor_tab==EditorTab::Warp {
                        if let Some(clip)=context.selected_audio.as_ref().filter(|c|c.warp_enabled) {
                            let x=wave_point(layout.view,pointer);self.wave_selection=Some([x,x]);
                            if let Some(markers)=clip.warp["markers"].as_array() {
                                let nearest=markers.iter().enumerate().filter_map(|(i,m)|{let mx=(m["source_frame"].as_f64()? -clip.source_offset as f64)/clip.frames.max(1)as f64;let dist=(mx-x).abs()*(layout.view.w-16.).max(1.);(dist<=9.).then_some((i,dist))}).min_by(|a,b|a.1.total_cmp(&b.1)).map(|p|p.0);
                                self.warp_selected=nearest;
                                if let Some(index)=nearest.filter(|i|*i>0&&*i+1<markers.len()){self.gesture=Some(Gesture::WarpMarker{clip:clip.clone(),revision:context.revision,index,markers:clip.warp["markers"].clone(),panel});}
                            }
                        }
                    } else if self.page == Page::ClipEditor
                        && self.editor_tab == EditorTab::Spectral
                    {
                        if let Some(clip) = &context.selected_audio {
                            let [x, f] = selection_point(layout.view, pointer);
                            self.spectral_selection = Some(SpectralSelection {
                                x0: x,
                                x1: x,
                                f0: f,
                                f1: f,
                            });
                            self.gesture = Some(Gesture::Spectral { clip: clip.clone() });
                        }
                    }
                }
            }
        }
        if let Some(gesture) = self.gesture.clone() {
            match gesture {
                Gesture::Button(id) => {
                    if released {
                        self.gesture = None;
                        if input
                            .pointer
                            .is_some_and(|p| layout.hit(p).is_some_and(|c| c.id == id))
                        {
                            self.activate(id, context, &mut result);
                        }
                    }
                }
                Gesture::Fader { id, offset } => {
                    if input.left && !pressed {
                        if let Some(pointer) = input.pointer {
                            let (top, usable) = fader_travel(layout.content.y, layout.content.h);
                            let value = (1. - (pointer[1] + offset - top) / usable).clamp(0., 1.);
                            let gain = fader_to_gain(value);
                            if context
                                .tracks
                                .iter()
                                .find(|track| track.id == id)
                                .is_some_and(|track| track.gain as f32 != gain as f32)
                            {
                                result.push(Action::SetTrackGain {
                                    revision: context.revision,
                                    id,
                                    gain,
                                });
                            }
                        }
                    } else if !input.left {
                        self.gesture = None;
                    }
                }
                Gesture::EditorBar{zoom,anchor,clip,revision,panel:original}=>{
                    if panel!=original||context.revision!=revision||context.selected_audio.as_ref()!=Some(&clip)||input.pointer.is_none(){self.gesture=None;}
                    else if let Some(p)=input.pointer {let x=((p[0]-layout.view.x)/layout.view.w).clamp(0.,1.);let x=if zoom{x}else{self.wave_view[0]+x*(self.wave_view[1]-self.wave_view[0])};let span=[anchor.min(x),anchor.max(x)];
                        if released {if zoom {self.wave_view=if (span[1]-span[0])*layout.view.w>3. {let width=(span[1]-span[0]).max(1./clip.frames.max(1) as f64).min(1.);let start=span[0].min(1.-width);[start,start+width]}else{[0.,1.]};}else if span[1]>span[0]{self.wave_loop=Some(span);let duration=clip.frames as f64/clip.sample_rate.max(1)as f64;result.push(Action::EditorLoop{start:clip.start_seconds+span[0]*duration,end:clip.start_seconds+span[1]*duration});}self.gesture=None;}
                    }
                }
                Gesture::Wave{clip,revision,anchor,panel:original}=>{
                    if context.revision!=revision||context.selected_audio.as_ref().map(|c|&c.id)!=Some(&clip.id)||panel!=original||input.pointer.is_none(){self.gesture=None;self.wave_selection=None;}
                    else if let Some(p)=input.pointer {self.wave_selection=Some([anchor,self.wave_view[0]+wave_point(layout.view,p)*(self.wave_view[1]-self.wave_view[0])]);}
                    if released{self.gesture=None;}
                }
                Gesture::WarpMarker{clip,revision,index,mut markers,panel:original}=>{
                    if context.revision!=revision||context.selected_audio.as_ref().map(|c|&c.id)!=Some(&clip.id)||panel!=original||input.pointer.is_none(){self.gesture=None;}
                    else if let Some(p)=input.pointer {
                        let value=clip.source_offset as f64+wave_point(layout.view,p)*clip.frames as f64;
                        let low=markers[index-1]["source_frame"].as_f64().unwrap()+1.;let high=markers[index+1]["source_frame"].as_f64().unwrap()-1.;
                        if low<=high{markers[index]["source_frame"]=serde_json::json!(value.clamp(low,high));}
                        if released {result.push(Action::ClipEdit{revision,id:clip.id.clone(),hook:"WorkstationDataSystem.SetWarpMap",input:serde_json::json!({"markers":markers,"transpose_semitones":clip.warp["transpose_semitones"].as_f64().unwrap_or(0.)})});self.gesture=None;}
                        else{self.gesture=Some(Gesture::WarpMarker{clip,revision,index,markers,panel:original});}
                    }
                }
                Gesture::Spectral { clip } => {
                    if context.selected_audio.as_ref().is_none_or(|c| {
                        c.id != clip.id
                            || c.source_offset != clip.source_offset
                            || c.frames != clip.frames
                    }) {
                        self.gesture = None;
                        self.spectral_selection = None;
                    } else if let Some(pointer) = input.pointer {
                        let [x, f] = selection_point(layout.view, pointer);
                        if let Some(s) = &mut self.spectral_selection {
                            s.x1 = x;
                            s.f1 = f;
                        }
                    }
                    if released {
                        self.gesture = None;
                    }
                }
                Gesture::LoopSelection { anchor } => {
                    if let Some(pointer) = input.pointer {
                        if let Some(frame) = self
                            .loopback
                            .frame_at_fraction((pointer[0] - layout.view.x) / layout.view.w)
                        {
                            self.loopback.select(anchor, frame);
                        }
                    }
                    if released {
                        self.gesture = None;
                    }
                }
                Gesture::LoopCopy => {
                    if self.loopback.selection().is_none() {
                        self.gesture = None;
                    } else if released {
                        self.gesture = None;
                        if let (Some(pointer), Some(audio)) =
                            (input.pointer, self.loopback.copy_selection())
                        {
                            result.push(Action::CopyLoopback {
                                audio,
                                pointer,
                                snap: !input.command,
                            });
                        }
                    }
                }
            }
        }
        result
    }
    fn activate(&mut self, id: ControlId, context: &Context, result: &mut Vec<Action>) {
        match id {
            ControlId::ClipOutput=>{if let Some(c)=context.selected_audio.as_ref().filter(|c|c.trackless){
                let choices:Vec<_>=std::iter::once("OFF").chain(context.buses.iter().filter(|b|b["kind"]=="matrix"||b["kind"]=="channel").filter_map(|b|b["id"].as_str())).collect();
                let index=choices.iter().position(|id|*id==c.output).unwrap_or(0);
                result.push(Action::ClipEdit{revision:context.revision,id:c.id.clone(),hook:"WorkstationDataSystem.SetClipRouting",input:serde_json::json!({"output":choices[(index+1)%choices.len()]})});
            }},
            ControlId::SelectPage(index) => {
                self.page = Page::from_index(index);
                result.push(Action::PageChanged(self.page));
            }
            ControlId::SelectPluginTrack(index) if index < context.plugin_track_count => result.push(Action::SelectPluginTrack(index)),
            ControlId::SelectPluginTrack(_) => {},
            ControlId::NextSoundtrack => result.push(Action::NextSoundtrack),
            ControlId::SelectTrack(id)=>result.push(Action::SelectTrack(id)),
            ControlId::SelectBus(id)=>result.push(Action::SelectBus(id)),
            ControlId::NextSpeaker => result.push(Action::NextSpeaker),
            ControlId::RecallAudio => result.push(Action::RecallAudio {
                seconds: self.loopback.memory_seconds(),
            }),
            ControlId::RecallMidi => result.push(Action::RecallMidi {
                seconds: self.loopback.memory_seconds(),
            }),
            ControlId::Memory => {
                let _ = self.loopback.cycle_memory();
            }
            ControlId::Source => result.push(Action::SetCaptureSource(
                if self.loopback.source() == Source::Microphone {
                    Source::Player
                } else {
                    Source::Microphone
                },
            )),
            ControlId::ClearBuffer => {
                self.loopback.clear();
                result.push(Action::ClearCapture);
            }
            ControlId::Waveform => {
                self.editor_tab = EditorTab::Waveform;
            }
            ControlId::Warp => {
                self.editor_tab = EditorTab::Warp;
            }
            ControlId::Spectral => {
                // Temporarily disabled; retain spectral implementation and edits.
                return;
            }
            ControlId::Strength => {
                self.attenuation_db = cycle(self.attenuation_db, &[-6., -12., -24., -48., -80.]);
            }
            ControlId::Feather => {
                self.feather = cycle(self.feather, &[0.04, 0.12, 0.24, 0.40]);
            }
            ControlId::ClipGainDown|ControlId::ClipGainReset|ControlId::ClipGainUp=>{
                if let Some(c)=&context.selected_audio{let gain=match id{ControlId::ClipGainReset=>1.,ControlId::ClipGainDown=>c.gain/10f64.powf(1./20.),_=>c.gain.max(0.001)*10f64.powf(1./20.)}.clamp(0.,16.);result.push(Action::ClipEdit{revision:context.revision,id:c.id.clone(),hook:"WorkstationDataSystem.SetClipGain",input:serde_json::json!({"gain":gain})});}
            }
            ControlId::WaveTrim|ControlId::WaveSplit=>{
                if let (Some(c),Some(range))=(&context.selected_audio,self.wave_selection){if id==ControlId::WaveSplit&&!(range[0]>0.&&range[0]<1.){return;}let duration=c.frames as f64/c.sample_rate as f64;
                    let (hook,input)=if id==ControlId::WaveTrim{("WorkstationDataSystem.TrimAudioClip",serde_json::json!({"start_fraction":range[0].min(range[1]),"end_fraction":range[0].max(range[1])}))}else{("WorkstationDataSystem.SplitClip",serde_json::json!({"time_seconds":c.start_seconds+duration*range[0]}))};
                    result.push(Action::ClipEdit{revision:context.revision,id:c.id.clone(),hook,input});}
            }
            ControlId::WarpPitchDown|ControlId::WarpPitchUp|ControlId::WarpAdd|ControlId::WarpRemove=>{
                if let Some(c)=&context.selected_audio {if let Some(mut markers)=c.warp["markers"].as_array().cloned(){
                    let mut pitch=c.warp["transpose_semitones"].as_f64().unwrap_or(0.);
                    match id {ControlId::WarpPitchDown=>pitch=(pitch-1.).max(-24.),ControlId::WarpPitchUp=>pitch=(pitch+1.).min(24.),
                        ControlId::WarpRemove=>{if let Some(i)=self.warp_selected.filter(|i|*i>0&&*i+1<markers.len()){markers.remove(i);self.warp_selected=None;}},
                        _=>{if let Some(range)=self.wave_selection{let source=c.source_offset as f64+range[0]*c.frames as f64;
                            if let Some(i)=markers.windows(2).position(|p|source>p[0]["source_frame"].as_f64().unwrap()+1.&&source<p[1]["source_frame"].as_f64().unwrap()-1.){let a=&markers[i];let b=&markers[i+1];let f=(source-a["source_frame"].as_f64().unwrap())/(b["source_frame"].as_f64().unwrap()-a["source_frame"].as_f64().unwrap());let beat=a["destination_beat"].as_f64().unwrap()+(b["destination_beat"].as_f64().unwrap()-a["destination_beat"].as_f64().unwrap())*f;markers.insert(i+1,serde_json::json!({"source_frame":source,"destination_beat":beat,"manual":true}));self.warp_selected=Some(i+1);}}
                        }}
                    result.push(Action::ClipEdit{revision:context.revision,id:c.id.clone(),hook:"WorkstationDataSystem.SetWarpMap",input:serde_json::json!({"markers":markers,"transpose_semitones":pitch})});
                }}
            }
            ControlId::WarpToggle => {
                if let Some(c) = &context.selected_audio {
                    result.push(Action::SetClipWarp {
                        revision: context.revision,
                        id: c.id.clone(),
                        enabled: !c.warp_enabled,
                    });
                }
            }
            ControlId::Attenuate => {
                if let (Some(c), Some(s)) = (&context.selected_audio, self.spectral_selection) {
                    let mask = Mask {
                        start: c
                            .source_offset
                            .saturating_add((s.x0.min(s.x1) * c.frames as f64).floor() as u64),
                        end: c
                            .source_offset
                            .saturating_add((s.x0.max(s.x1) * c.frames as f64).ceil() as u64),
                        low_hz: norm_to_hz(s.f0.min(s.f1) as f32, c.sample_rate),
                        high_hz: norm_to_hz(s.f0.max(s.f1) as f32, c.sample_rate),
                        attenuation_db: self.attenuation_db,
                        feather: self.feather,
                    };
                    if let Ok(mask) = mask.validated() {
                        result.push(Action::AddSpectralMask {
                            revision: context.revision,
                            id: c.id.clone(),
                            mask,
                        });
                    }
                }
            }
            ControlId::UndoMask => {
                if let Some(c) = &context.selected_audio {
                    result.push(Action::UndoSpectralMask {
                        revision: context.revision,
                        id: c.id.clone(),
                    });
                }
            }
            ControlId::ClearMasks => {
                if let Some(c) = &context.selected_audio {
                    result.push(Action::ClearSpectralMasks {
                        revision: context.revision,
                        id: c.id.clone(),
                    });
                }
            }
            ControlId::LoopbackTab | ControlId::Fader(_) => {}
        }
    }
}
fn wave_point(rect:Rect,p:[f64;2])->f64{((p[0]-rect.x-8.)/(rect.w-16.).max(1.)).clamp(0.,1.)}
fn selection_point(rect: Rect, p: [f64; 2]) -> [f64; 2] {
    [
        ((p[0] - rect.x) / rect.w.max(1.)).clamp(0., 1.),
        (1. - (p[1] - rect.y) / rect.h.max(1.)).clamp(0., 1.),
    ]
}
fn cycle(current: f32, values: &[f32]) -> f32 {
    let nearest = values
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - current).abs().total_cmp(&(**b - current).abs()))
        .unwrap()
        .0;
    values[(nearest + 1) % values.len()]
}
pub(crate) fn fader_to_gain(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.02 {
        0.
    } else {
        10f64.powf((-60. + (value.clamp(0., 1.) - 0.02) / 0.98 * 65.) / 20.)
    }
}
pub(crate) fn gain_to_fader(gain: f64) -> f64 {
    if !gain.is_finite() || gain <= 0. {
        0.
    } else {
        (0.02 + ((20. * gain.log10()).clamp(-60., 5.) + 60.) / 65. * 0.98).clamp(0., 1.)
    }
}
// GemFader_Ivory motion_components.travel_y_m, scaled with its casing.
fn fader_travel(y: f64, content_height: f64) -> (f64, f64) {
    let height = (content_height - 22.).max(0.);
    (y + height * (0.5 - 0.0655 / 0.18), (height * (0.109 / 0.18)).max(1.))
}

/// Parameters come from DecibelMeterContext, so existing attack/decay settings
/// stay authoritative. Input is the audio engine's normalized peak level.
pub(crate) fn meter_step(
    current: f64,
    target: f64,
    dt: f64,
    attack_speed: f64,
    decay_per_second: f64,
) -> f64 {
    if ![current, target, dt, attack_speed, decay_per_second]
        .iter()
        .all(|v| v.is_finite())
    {
        return 0.;
    }
    let dt = dt.max(0.);
    (if target > current {
        current + (target - current) * attack_speed.max(0.) * dt
    } else {
        target.max(current - decay_per_second.max(0.) * dt)
    })
    .clamp(0., 1.)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clip_editor_fills_face_and_keeps_controls_on_top_bevel() {
        let c = context();
        for tab in [EditorTab::Waveform, EditorTab::Warp, EditorTab::Spectral] {
            let s = State { page: Page::ClipEditor, editor_tab: tab, ..Default::default() };
            let p = panel();
            let l = s.layout(p, &c);
            let inset = p.h * 0.155;
            assert_eq!(l.content.x, p.x + inset);
            assert_eq!(l.content.w, p.w - 2. * inset);
            assert_eq!(l.view.x, l.content.x);
            assert_eq!(l.view.w, l.content.w);
            assert_eq!(l.view.y + l.view.h, p.y + p.h - inset);
            for control in &l.controls {
                assert!(control.rect.y >= p.y);
                assert!(control.rect.y + control.rect.h <= l.content.y);
                assert!(control.rect.x >= l.content.x);
                assert!(control.rect.x + control.rect.w <= l.content.x + l.content.w);
            }
            for pair in l.controls.windows(2) {
                assert!(pair[0].rect.x + pair[0].rect.w <= pair[1].rect.x);
            }
        }
    }
    #[test] fn normal_track_header_and_fader_both_select_the_track(){
        let c=context();let mut s=State::default();
        assert!(matches!(click(&mut s,&c,ControlId::SelectTrack("audio".into())).as_slice(),[Action::SelectTrack(id)] if id=="audio"));
        let knob=s.layout(panel(),&c).faders[0].knob;
        let actions=s.update(&Input{pointer:Some([knob.x+knob.w/2.,knob.y+knob.h/2.]),left:true,focused:true,..Default::default()},panel(),&c);
        assert!(actions.iter().any(|a|matches!(a,Action::SelectTrack(id) if id=="audio")));
    }
    fn panel() -> Rect {
        Rect {
            x: 0.,
            y: 500.,
            w: 1100.,
            h: 300.,
        }
    }
    pub(super) fn context() -> Context {
        Context {
            revision: 7,
            tracks: vec![Track {
                id: "audio".into(),
                label: "AUDIO 1".into(),
                gain: 1.,
                meter: [0.2, 0.3],
            }],
            plugin_track_count: 2,
            selected_audio: Some(AudioClip {trackless:false,output:String::new(),
                id: "clip".into(),
                source_offset: 1000,
                frames: 48000,
                sample_rate: 48000,
                gain:1.,start_seconds:0.,warp:serde_json::Value::Null,
                warp_enabled: false,
                spectral_masks: 1,
            }),
            ..Default::default()
        }
    }
    fn click(s: &mut State, c: &Context, id: ControlId) -> Vec<Action> {
        let r = s
            .layout(panel(), c)
            .controls
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .rect;
        let p = Some([r.x + r.w / 2., r.y + r.h / 2.]);
        s.update(
            &Input {
                pointer: p,
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            c,
        );
        s.update(
            &Input {
                pointer: p,
                focused: true,
                ..Default::default()
            },
            panel(),
            c,
        )
    }
    #[test]
    fn numbered_pages_select_directly_and_skip_buffer() {
        let c = context();
        let mut s = State::default();
        for page in [Page::ClipEditor, Page::Mixer, Page::Plugins] {
            assert!(matches!(click(&mut s, &c, ControlId::SelectPage(page.index())).as_slice(),
                [Action::PageChanged(p)] if *p == page));
            let layout = s.layout(panel(), &c);
            let buttons = &layout.controls[..3];
            assert_eq!(buttons.iter().map(|b| b.label.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
            assert_eq!(buttons.iter().filter(|b| b.active).count(), 1);
            assert!(buttons.iter().all(|b| b.enabled && b.rect.w > 0. && b.rect.w <= 43.2 && b.rect.h > 0. && b.rect.h <= 43.2));
            assert!(buttons.iter().find(|b| b.active).unwrap().id == ControlId::SelectPage(page.index()));
        }
    }
    #[test]
    fn plugin_track_gems_select_directly_and_scroll_without_overflow() {
        let mut c = context();
        c.plugin_track_count = 8;
        let mut s = State { page: Page::Plugins, ..Default::default() };
        for index in [2, 0, 1] {
            assert!(matches!(click(&mut s, &c, ControlId::SelectPluginTrack(index)).as_slice(),
                [Action::SelectPluginTrack(i)] if *i == index));
        }
        let row = s.layout(panel(), &c).controls[3].rect;
        let input = Input { pointer: Some([row.x + 11., row.y + 11.]), wheel: 1., focused: true, ..Default::default() };
        s.update(&input, panel(), &c);
        let layout = s.layout(panel(), &c);
        assert_eq!(layout.controls.len(), 6);
        assert_eq!(layout.controls[3].id, ControlId::SelectPluginTrack(3));
        assert!(layout.controls.iter().all(|b| b.rect.w == 43.2 && b.rect.h == 43.2));
        assert!(matches!(click(&mut s, &c, ControlId::SelectPluginTrack(5)).as_slice(), [Action::SelectPluginTrack(5)]));
        c.plugin_track_index = 7;
        s.sync(&c);
        let layout = s.layout(panel(), &c);
        assert_eq!(layout.controls.len(), 5);
        assert!(layout.controls.iter().any(|b| b.id == ControlId::SelectPluginTrack(7) && b.active));
        c.plugin_track_count = 0;
        s.sync(&c);
        assert_eq!(s.layout(panel(), &c).controls.len(), 3);
    }
    #[test]
    fn navigation_requires_press_origin_and_release_inside() {
        let c = context();
        let mut s = State::default();
        let p = Some([90., 550.]);
        s.update(
            &Input {
                pointer: Some([0., 0.]),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        assert!(
            s.update(
                &Input {
                    pointer: p,
                    focused: true,
                    ..Default::default()
                },
                panel(),
                &c
            )
            .is_empty()
        );
        assert_eq!(s.page, Page::Mixer);
    }
    #[test]
    fn fader_cap_stays_on_authored_rail_when_panel_resizes() {
        for height in [180., 300., 620.] {
            let mut c = context();
            let mut p = panel();
            p.h = height;
            for normalized in [0., 0.5, 1.] {
                c.tracks[0].gain = fader_to_gain(normalized);
                let f = State::default().layout(p, &c).faders[0].clone();
                let center = f.knob.y + f.knob.h * 0.5;
                let local_y = (f.housing.y + f.housing.h * 0.5 - center) / f.housing.h * 0.18;
                assert!((local_y - (-0.0435 + f.normalized * 0.109)).abs() < 1e-10);
                assert!(f.knob.y >= f.housing.y && f.knob.y + f.knob.h <= f.housing.y + f.housing.h);
            }
        }
    }
    #[test]
    fn fader_uses_original_gain_law_and_identity() {
        assert_eq!(fader_to_gain(0.02), 0.);
        assert!((fader_to_gain(1.) - 10f64.powf(5. / 20.)).abs() < 1e-10);
        assert!((fader_to_gain(gain_to_fader(1.)) - 1.).abs() < 1e-10);
        let mut c = context();
        let mut s = State::default();
        let knob = s.layout(panel(), &c).faders[0].knob;
        let x = knob.x + 12.5;
        s.update(
            &Input {
                pointer: Some([x, knob.y + 12.5]),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        let out = s.update(
            &Input {
                pointer: Some([x, 550.]),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        assert!(
            matches!(out.as_slice(),[Action::SetTrackGain{revision:7,id,gain}] if id=="audio"&&*gain>1.)
        );
        if let Action::SetTrackGain{gain,..}=&out[0] {
            c.tracks.iter_mut().find(|t|t.id=="audio").unwrap().gain=(*gain as f32) as f64;
        }
        assert!(s.update(&Input{pointer:Some([x,550.]),left:true,focused:true,..Default::default()},panel(),&c).is_empty());
    }
    #[test]
    fn press_uses_physical_pressure_and_stationary_grip_does_not_edit() {
        let c = context();
        let mut s = State::default();
        let knob = s.layout(panel(), &c).faders[0].knob;
        let input = Input {
            pointer: Some([knob.x + 12.5, knob.y + 12.5]),
            left: true,
            focused: true,
            ..Default::default()
        };
        assert!(matches!(s.update(&input, panel(), &c).as_slice(),[Action::SelectTrack(id)] if id=="audio"));
        s.advance_pressure(0.075);
        let value = s
            .layout(panel(), &c)
            .controls
            .into_iter()
            .find(|c| matches!(c.id, ControlId::Fader(_)))
            .unwrap()
            .pressure;
        assert!((value - 0.25).abs() < 1e-9);
        assert!(s.update(&input, panel(), &c).is_empty());
        s.update(
            &Input {
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        s.advance_pressure(0.15);
        assert!(
            s.layout(panel(), &c)
                .controls
                .iter()
                .all(|c| c.pressure == 0.)
        );
    }
    #[test]
    fn focus_loss_cancels_drag_and_requires_release() {
        let c = context();
        let mut s = State::default();
        let r = s.layout(panel(), &c).faders[0].knob;
        s.update(
            &Input {
                pointer: Some([r.x + 2., r.y + 2.]),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        assert!(
            s.update(
                &Input {
                    left: true,
                    focused: false,
                    ..Default::default()
                },
                panel(),
                &c
            )
            .is_empty()
        );
        assert!(
            s.update(
                &Input {
                    pointer: Some([200., 600.]),
                    left: true,
                    focused: true,
                    ..Default::default()
                },
                panel(),
                &c
            )
            .is_empty()
        );
        assert!(s.consumes_pointer());
        s.update(
            &Input {
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        assert!(!s.consumes_pointer());
    }
    #[test]
    fn original_spectral_control_cycles_and_source_coordinates() {
        let c = context();
        let mut s = State {
            page: Page::ClipEditor,
            ..Default::default()
        };
        click(&mut s, &c, ControlId::Spectral);
        assert_eq!(s.editor_tab, EditorTab::Waveform);
        // Retained implementation remains testable while its UI entry is disabled.
        s.editor_tab = EditorTab::Spectral;
        click(&mut s, &c, ControlId::Strength);
        assert_eq!(s.attenuation_db, -48.);
        click(&mut s, &c, ControlId::Feather);
        assert_eq!(s.feather, 0.24);
        let r = s.layout(panel(), &c).view;
        s.update(
            &Input {
                pointer: Some([r.x + r.w * 0.8, r.y + r.h * 0.2]),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        s.update(
            &Input {
                pointer: Some([r.x + r.w * 0.2, r.y + r.h * 0.8]),
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        let out = click(&mut s, &c, ControlId::Attenuate);
        assert!(
            matches!(out.as_slice(),[Action::AddSpectralMask{revision:7,id,mask}] if id=="clip"&&mask.start==10600&&mask.end==39400&&mask.low_hz<mask.high_hz)
        );
    }
    #[test]
    fn selection_never_leaks_to_new_audio_clip() {
        let mut c = context();
        let mut s = State {
            page: Page::ClipEditor,
            editor_tab: EditorTab::Spectral,
            ..Default::default()
        };
        s.sync(&c);
        s.spectral_selection = Some(SpectralSelection {
            x0: 0.,
            x1: 1.,
            f0: 0.,
            f1: 1.,
        });
        c.selected_audio.as_mut().unwrap().id = "other".into();
        s.sync(&c);
        assert!(s.spectral_selection.is_none());
    }
    #[test]
    fn loopback_drag_copies_original_span_with_command_disabling_snap() {
        let c = context();
        let mut s = State {
            page: Page::Loopback,
            ..Default::default()
        };
        s.loopback.configure(Source::Microphone, 8000).unwrap();
        s.loopback
            .drain(Source::Microphone, 0, 100, |i| Some([i as f32, 0.]))
            .unwrap();
        let r = s.layout(panel(), &c).view;
        let p = |x| Some([r.x + r.w * x, r.y + 5.]);
        s.update(
            &Input {
                pointer: p(0.2),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        s.update(
            &Input {
                pointer: p(0.6),
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        let selected = s.loopback.selection().unwrap();
        s.update(
            &Input {
                pointer: p(0.4),
                left: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        let out = s.update(
            &Input {
                pointer: Some([300., 100.]),
                command: true,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        assert!(
            matches!(out.as_slice(),[Action::CopyLoopback{audio,snap:false,..}] if audio.absolute_start==selected.start&&audio.interleaved.len()==(selected.end-selected.start)as usize)
        );
    }
    #[test]
    fn waveform_gain_and_edit_controls_use_selected_clip() {
        let mut c = context();
        let mut s = State { page: Page::ClipEditor, ..Default::default() };
        let out = click(&mut s, &c, ControlId::ClipGainUp);
        assert!(matches!(&out[0], Action::ClipEdit {revision:7,id,hook,input}
            if id=="clip" && *hook=="WorkstationDataSystem.SetClipGain"
                && (input["gain"].as_f64().unwrap()-10f64.powf(1./20.)).abs()<1e-10));
        s.wave_selection=Some([0.,0.]);
        assert!(click(&mut s,&c,ControlId::WaveSplit).is_empty());
        s.wave_selection=Some([0.25,0.75]);
        let out=click(&mut s,&c,ControlId::WaveTrim);
        assert!(matches!(&out[0],Action::ClipEdit{hook,input,..}
            if *hook=="WorkstationDataSystem.TrimAudioClip" && input["start_fraction"]==0.25 && input["end_fraction"]==0.75));
        c.selected_audio.as_mut().unwrap().warp_enabled=true;
        assert!(click(&mut s,&c,ControlId::WaveTrim).is_empty());
    }
    #[test]
    fn warp_marker_actions_require_an_editable_selection() {
        let mut c=context();
        let clip=c.selected_audio.as_mut().unwrap();
        clip.warp_enabled=true;
        clip.warp=serde_json::json!({"markers":[
            {"source_frame":1000.,"destination_beat":0.},
            {"source_frame":25000.,"destination_beat":1.},
            {"source_frame":49000.,"destination_beat":2.}]});
        let mut s=State{page:Page::ClipEditor,editor_tab:EditorTab::Warp,..Default::default()};
        assert!(click(&mut s,&c,ControlId::WarpAdd).is_empty());
        s.wave_selection=Some([0.25,0.25]);
        let out=click(&mut s,&c,ControlId::WarpAdd);
        assert!(matches!(&out[0],Action::ClipEdit{hook,input,..}
            if *hook=="WorkstationDataSystem.SetWarpMap" && input["markers"].as_array().unwrap().len()==4));
        s.warp_selected=Some(0);
        assert!(click(&mut s,&c,ControlId::WarpRemove).is_empty());
        s.warp_selected=Some(1);
        let out=click(&mut s,&c,ControlId::WarpRemove);
        assert!(matches!(&out[0],Action::ClipEdit{input,..} if input["markers"].as_array().unwrap().len()==2));
    }
    #[test]
    fn disabled_and_clipped_controls_cannot_activate() {
        let mut c = context();
        c.selected_audio = None;
        let mut s = State {
            page: Page::ClipEditor,
            editor_tab: EditorTab::Warp,
            ..Default::default()
        };
        for tab in [EditorTab::Waveform, EditorTab::Warp, EditorTab::Spectral] {
            s.editor_tab=tab;
            let layout=s.layout(panel(),&c);
            assert!(layout.controls.iter().all(|control| matches!(control.id,
                ControlId::SelectPage(_) | ControlId::Waveform | ControlId::Warp | ControlId::Spectral)));
        }
        let small = Rect {
            x: 0.,
            y: 0.,
            w: 80.,
            h: 200.,
        };
        let layout = s.layout(small, &c);
        assert!(layout.hit([100., 50.]).is_none());
    }
    #[test]
    fn mixer_meter_labels_and_legacy_scroll_bounds() {
        let mut c = context();
        c.buses=super::super::buses::Buses::default().snapshot().as_array().unwrap().clone();
        let mut s = State::default();
        assert_eq!(
            s.layout(panel(), &c)
                .master_meters
                .iter()
                .map(|m| m.0.as_str())
                .collect::<Vec<_>>(),
            vec!["ML", "MS", "MFF", "MR", "MX1L", "MX1R"]
        );
        s.update(
            &Input {
                pointer: Some([200., 600.]),
                wheel: 1.,
                focused: true,
                ..Default::default()
            },
            panel(),
            &c,
        );
        assert_eq!(s.mixer_scroll, 40.);
    }
}

#[cfg(test)]mod capture_ownership_regressions{
 use super::*;
 #[test]fn blocking_foreign_held_input_does_not_steal_timeline_or_piano_capture(){let mut s=State::default();s.cancel(true);assert!(!s.consumes_pointer());assert!(s.blocked_until_release);s.cancel(false);assert!(!s.consumes_pointer());}
 #[test]fn cancelling_our_own_gesture_keeps_release_owned(){let mut s=State::default();s.gesture=Some(Gesture::Button(ControlId::SelectPage(1)));s.cancel(true);assert!(s.consumes_pointer());s.cancel(false);assert!(!s.consumes_pointer());}
}

#[cfg(test)]mod clip_editor_controls {
 use super::*;use serde_json::json;
 #[test]fn waveform_command_wheel_reaches_one_sample_and_keeps_pointer_time(){
 let c=context();let mut s=State{page:Page::ClipEditor,..Default::default()};s.sync(&c);let v=s.layout(panel(),&c).view;
 let p=[v.x+v.w*0.5,v.y+v.h*0.5];let mut input=pointer(p,false);input.command=true;input.wheel= -1.;
 for _ in 0..150{s.update(&input,panel(),&c);}
 assert!((s.wave_view[1]-s.wave_view[0]-1./48000.).abs()<1e-12);
 assert!(((s.wave_view[0]+s.wave_view[1])*0.5-0.5).abs()<1e-12);
 input.command=false;input.wheel=1.;let before=s.wave_view;s.update(&input,panel(),&c);assert!(s.wave_view[0]>before[0]);
 assert!(s.wave_selection.is_none());
 }
 #[test]fn waveform_bars_zoom_selection_and_transport_loop(){
 let c=context();let mut s=State{page:Page::ClipEditor,..Default::default()};s.sync(&c);let v=s.layout(panel(),&c).view;
 let point=|f:f64,y:f64|[v.x+v.w*f,y];
 s.update(&pointer(point(0.25,v.y-35.),true),panel(),&c);s.update(&pointer(point(0.75,v.y-35.),false),panel(),&c);assert_eq!(s.wave_view,[0.25,0.75]);
 s.update(&pointer([v.x+8.,v.y+v.h*0.5],true),panel(),&c);s.update(&pointer([v.x+v.w-8.,v.y+v.h*0.5],false),panel(),&c);assert_eq!(s.wave_selection,Some([0.25,0.75]));
 s.update(&pointer(point(0.,v.y-12.),true),panel(),&c);let out=s.update(&pointer(point(1.,v.y-12.),false),panel(),&c);assert!(matches!(out.first(),Some(Action::EditorLoop{start,end})if *start==2.25&&*end==2.75));
 s.update(&pointer(point(0.5,v.y-35.),true),panel(),&c);s.update(&pointer(point(0.5,v.y-35.),false),panel(),&c);assert_eq!(s.wave_view,[0.,1.]);
 }
 fn context()->Context{Context{revision:7,selected_audio:Some(AudioClip{trackless:false,output:String::new(),id:"c".into(),source_offset:0,frames:48000,sample_rate:48000,gain:1.,start_seconds:2.,warp_enabled:true,spectral_masks:0,warp:json!({"markers":[{"source_frame":0.,"destination_beat":0.},{"source_frame":24000.,"destination_beat":1.},{"source_frame":48000.,"destination_beat":2.}],"transpose_semitones":0.})}),..Default::default()}}
 fn panel()->Rect{Rect{x:0.,y:0.,w:900.,h:300.}}
 fn pointer(p:[f64;2],left:bool)->Input{Input{pointer:Some(p),left,focused:true,..Default::default()}}
 fn click(s:&mut State,c:&Context,id:ControlId)->Vec<Action>{let r=s.layout(panel(),c).controls.into_iter().find(|x|x.id==id).unwrap().rect;let p=[r.x+r.w*0.5,r.y+r.h*0.5];s.update(&pointer(p,true),panel(),c);s.update(&pointer(p,false),panel(),c)}
 #[test]fn waveform_selection_gain_trim_split_use_revisioned_document_actions(){let mut c=context();c.selected_audio.as_mut().unwrap().warp_enabled=false;let mut s=State{page:Page::ClipEditor,..Default::default()};let out=click(&mut s,&c,ControlId::ClipGainDown);assert!(matches!(&out[0],Action::ClipEdit{revision:7,hook:"WorkstationDataSystem.SetClipGain",input,..}if (input["gain"].as_f64().unwrap()-10f64.powf(-1./20.)).abs()<1e-12));let view=s.layout(panel(),&c).view;let p=[view.x+8.+(view.w-16.)*0.25,view.y+view.h*0.5];s.update(&pointer(p,true),panel(),&c);s.update(&pointer([view.x+8.+(view.w-16.)*0.75,p[1]],false),panel(),&c);assert_eq!(s.wave_selection,Some([0.25,0.75]));let out=click(&mut s,&c,ControlId::WaveTrim);assert!(matches!(&out[0],Action::ClipEdit{hook:"WorkstationDataSystem.TrimAudioClip",input,..}if input["start_fraction"]==0.25&&input["end_fraction"]==0.75));let out=click(&mut s,&c,ControlId::WaveSplit);assert!(matches!(&out[0],Action::ClipEdit{hook:"WorkstationDataSystem.SplitClip",input,..}if input["time_seconds"]==2.25));}
 #[test]fn warp_drag_previews_then_commits_and_revision_change_cancels(){let c=context();let mut s=State{page:Page::ClipEditor,editor_tab:EditorTab::Warp,..Default::default()};let view=s.layout(panel(),&c).view;let p=[view.x+view.w*0.5,view.y+view.h*0.5];assert!(s.update(&pointer(p,true),panel(),&c).is_empty());assert!(s.update(&pointer([p[0]+20.,p[1]],true),panel(),&c).is_empty());assert!(s.warp_preview().is_some());let out=s.update(&pointer([p[0]+20.,p[1]],false),panel(),&c);assert!(matches!(&out[0],Action::ClipEdit{revision:7,hook:"WorkstationDataSystem.SetWarpMap",input,..}if input["markers"][1]["source_frame"].as_f64().unwrap()>24000.));s.update(&pointer(p,true),panel(),&c);let mut changed=c.clone();changed.revision+=1;assert!(s.update(&pointer(p,false),panel(),&changed).is_empty());assert!(s.warp_preview().is_none());let out=click(&mut s,&changed,ControlId::WarpPitchUp);assert!(matches!(&out[0],Action::ClipEdit{input,..}if input["transpose_semitones"]==1.));}
}

#[cfg(test)]mod waveform_bar_tests {
 use super::*;
 #[test]fn waveform_hit_fraction_matches_transparent_display_inset(){let r=Rect{x:20.,y:30.,w:216.,h:100.};assert_eq!(wave_point(r,[78.,50.]),0.25);assert_eq!(wave_point(r,[178.,50.]),0.75);assert_eq!(wave_point(r,[0.,50.]),0.);}
}

#[cfg(test)]mod trackless_routing_tests {
 use super::*;use serde_json::json;
 #[test]fn waveform_route_control_only_exists_for_trackless_audio_and_cycles_real_buses(){
  let mut context=super::tests::context();
  context.buses=vec![json!({"id":"matrix-L","kind":"matrix","input":"L"}),json!({"id":"channel","kind":"channel","name":"Channel Bus 1"})];
  let mut s=State{page:Page::ClipEditor,..Default::default()};
  let panel=Rect{x:0.,y:0.,w:1000.,h:400.};
  assert!(!s.layout(panel,&context).controls.iter().any(|c|c.id==ControlId::ClipOutput));
  let c=context.selected_audio.as_mut().unwrap();c.trackless=true;c.output="matrix-L".into();
  assert!(s.layout(panel,&context).controls.iter().any(|c|c.id==ControlId::ClipOutput&&c.label=="TO L"));
  let mut actions=Vec::new();s.activate(ControlId::ClipOutput,&context,&mut actions);
  assert!(matches!(&actions[0],Action::ClipEdit{hook:"WorkstationDataSystem.SetClipRouting",input,..}if input["output"]=="channel"));
  s.editor_tab=EditorTab::Warp;assert!(!s.layout(panel,&context).controls.iter().any(|c|c.id==ControlId::ClipOutput));
 }
}
