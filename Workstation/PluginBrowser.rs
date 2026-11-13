//! Cardinal's supported component/plug-in catalog rows and release-to-lane drag.
//! UIOnly labels and hit rectangles share this layout on the authored left gem.
use super::{
    browser,
    panel_layout::{self, Options, Viewport},
    panel_motion::Snapshot,
    piano_roll::Input,
};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    Component(&'static str),
    Plugin(Value),
    Audio(Value),
}
#[derive(Clone)]
pub(crate) struct Row {
    pub text: String,
    pub rect: [f64; 4],
    pub item: Option<Item>,
    pub header: bool,
}
pub(crate) struct Layout {
    pub screen: [f64; 4],
    pub clip: [f64; 4],
    pub content: [f64; 4],
    pub categories: [[f64; 4]; 4],
    pub back: [f64;4],
    pub search: [f64;4],
    pub imports: [[f64;4];2],
    pub query: String,
    search_focused:bool,
    pub rows: Vec<Row>,
    pub scale: f64,
    pub minimum: f64,
}
fn inside(r: [f64; 4], p: [f64; 2]) -> bool {
    p.iter().all(|n| n.is_finite())
        && p[0] >= r[0]
        && p[0] < r[0] + r[2]
        && p[1] >= r[1]
        && p[1] < r[1] + r[3]
}
pub(crate) fn layout(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
    catalog: &[Value],
    category: usize,
    scroll: f64,
) -> Result<Layout, String> {
    layout_filtered(view,motion,options,catalog,category,scroll,"")
}
pub(crate) fn layout_filtered(view:Viewport,motion:&Snapshot,options:Options,catalog:&[Value],category:usize,scroll:f64,query:&str)->Result<Layout,String>{
    let surface = panel_layout::resolved_surface(view, motion, options, 2)?;
    let mpp = 1.3 / view.height;
    let c = view.canvas_scale;
    let padding = 9.6 * c;
    let mut clip = [
        view.width * 0.5 + (surface.origin[0] - surface.size[0] * 0.5) / mpp + padding,
        view.height * 0.5 - (surface.origin[1] + surface.size[1] * 0.5) / mpp + padding,
        (surface.size[0] / mpp - 2. * padding).max(0.),
        (surface.size[1] / mpp - 2. * padding).max(0.),
    ];
    clip[1] += 32. * c;
    clip[3] = (clip[3] - 32. * c).max(0.);
    // One full-width page: categories at the root, contents after navigation.
    let row_height = 38.4*c;
    let header_height = 30.*c;
    let search=[clip[0],clip[1],clip[2],32.*c];
    let back=[clip[0],clip[1]+36.*c,clip[2],30.*c];
    let imports=std::array::from_fn(|i|[clip[0],clip[1]+clip[3]-(2-i) as f64*38.*c,clip[2],32.*c]);
    let content=[clip[0],clip[1]+72.*c,clip[2],(clip[3]-152.*c).max(0.)];
    let categories=std::array::from_fn(|i|if category==4 && query.is_empty() {[content[0],content[1]+i as f64*row_height,content[2],row_height]}else{[0.;4]});
    let mut specs: Vec<(String, Option<Item>, bool)> = Vec::new();
    if category == 0 || category == 1 || (category==4&&!query.is_empty()) {
        specs.push(("DAW COMPONENTS".into(), None, true));
        for (name, kind) in [
            ("Audio Track", "audio"),
            ("Midi Track", "midi"),
            ("CC Track", "cc"),
            ("Comp Lane", "comp"),
            ("Matrix Bus", "matrix"),
            ("Mix Bus", "mix"),
        ] {
            specs.push((name.into(), Some(Item::Component(kind)), false));
        }
    }
    if category == 0 || category == 2 || (category==4&&!query.is_empty()) {
        for (name, formats) in [
            ("STOCK PLUGINS", &["Stock"][..]),
            ("VST3 / VST2", &["VST3", "VST2"][..]),
            ("USER_CLAP", &["CLAP"][..]),
            ("USER_AUDIOUNIT", &["Audio Unit"][..]),
        ] {
            let entries: Vec<_> = catalog
                .iter()
                .filter(|p| formats.contains(&p["format"].as_str().unwrap_or("")))
                .collect();
            if category == 2 || !entries.is_empty() {
                specs.push((name.into(), None, true));
            }
            for p in entries {
                specs.push((
                    super::plugin_surface::label_text(p["name"].as_str().unwrap_or(""), 1024),
                    Some(Item::Plugin(p.clone())),
                    false,
                ));
            }
        }
    }
    if category == 0 || category == 3 || (category==4&&!query.is_empty()) {
        specs.push(("USER_SAMPLES".into(), None, true));
        for entry in catalog.iter().filter(|p| p["format"] == "WAV") {
            specs.push((super::plugin_surface::label_text(entry["name"].as_str().unwrap_or(""), 1024), Some(Item::Audio(entry.clone())), false));
        }
    }
    if !query.is_empty() {
        let query=query.to_lowercase();
        specs.retain(|(name,item,_)|item.is_some() && name.to_lowercase().contains(&query));
    }
    let height: f64 = specs
        .iter()
        .map(|(_, _, h)| if *h { header_height } else { row_height })
        .sum();
    let minimum = (content[3] - height).min(0.);
    let mut y = content[1] + scroll.clamp(minimum, 0.);
    let rows = specs
        .into_iter()
        .map(|(text, item, header)| {
            let h = if header { header_height } else { row_height };
            let row = Row {
                text,
                item,
                header,
                rect: [content[0], y, content[2], h],
            };
            y += h;
            row
        })
        .collect();
    Ok(Layout {
        screen: [0., 0., view.width, view.height],
        clip,
        content,
        categories,
        back, search, imports, query:query.into(),search_focused:false,
        rows,
        scale: c,
        minimum,
    })
}
impl Layout {
    pub(crate) fn enable_button(&self)->[f64;4] {
        [self.clip[0]+8.*self.scale,self.clip[1]+self.clip[3]*0.35,
         (self.clip[2]-16.*self.scale).max(0.),36.*self.scale]
    }
    pub(crate) fn locked_presentation(&self)->Value {
        let button=self.enable_button();
        json!([{"clip":self.clip,"labels":[
            {"text":"BROWSER PAUSED","rect":[button[0],button[1]-40.*self.scale,button[2],28.*self.scale],"size":14.*self.scale,"center":true,"color":[235,235,235]},
            {"text":"ENABLE BROWSER","rect":button,"size":16.*self.scale,"center":true,"color":[255,255,255]},
            {"text":"Load files and plugins","rect":[button[0],button[1]+44.*self.scale,button[2],26.*self.scale],"size":12.*self.scale,"center":true,"color":[210,210,210]}],
            "browser_faces":[{"rect":button,"radius":6.*self.scale,"color":[70.,115.,150.],"alpha":0.7}]}])
    }
    pub(crate) fn search_focus(mut self,active:bool)->Self{self.search_focused=active;self}
    pub(crate) fn hit(&self, p: [f64; 2]) -> Option<Item> {
        if !inside(self.screen, p) || !inside(self.content, p) {
            return None;
        }
        self.rows
            .iter()
            .find(|r| inside(r.rect, p))
            .and_then(|r| r.item.clone())
    }
    pub(crate) fn presentation(&self, category: usize, selected: Option<&Item>) -> Value {
        self.presentation_with_hover(category, selected, None)
    }
    /// Vst3BrowserPreviewSystem draws row faces below stable MiraText lettering.
    /// Hover follows the current scrolled rectangle, including fractional rows.
    pub(crate) fn presentation_with_hover(
        &self,
        category: usize,
        selected: Option<&Item>,
        pointer: Option<[f64; 2]>,
    ) -> Value {
        let pointer = pointer.filter(|p| inside(self.screen, *p));
        let mut labels = vec![
            json!({"text":if self.query.is_empty()&&!self.search_focused{"Search...".to_owned()}else{format!("Search: {}{}",self.query,if self.search_focused{"|"}else{""})},"rect":self.search,"size":15.6*self.scale,"color":[59,52,46]}),
            json!({"text":if category==4{"BROWSER"}else{"< BACK"},"rect":self.back,"size":14.4*self.scale,"color":[59,52,46]})];
        for (i,name) in ["IMPORT AUDIO","ADD PLUGINS"].iter().enumerate(){labels.push(json!({"text":name,"rect":self.imports[i],"size":14.4*self.scale,"center":true,"color":[59,52,46]}));}
        labels.retain(|label|label["rect"][2].as_f64().unwrap_or(0.)>0. && label["rect"][3].as_f64().unwrap_or(0.)>0.);
        let mut category_hover = Vec::new();
        let mut category_selected = Vec::new();
        for (i, name) in ["All", "Components", "Plugins", "Audio"].iter().enumerate() {
            let r = self.categories[i];
            if r[1] < self.clip[1] || r[1] + r[3] > self.clip[1] + self.clip[3] {
                continue;
            }
            let face = inset(r, 2. * self.scale, 3. * self.scale);
            if pointer.is_some_and(|p| inside(r, p) && inside(self.clip, p)) {
                category_hover.push(browser_face(face, 6. * self.scale, 0.28));
            }
            if i == category {
                category_selected.push(browser_face(face, 6. * self.scale, 0.76));
            }
            let r = [
                r[0] + 9.6 * self.scale,
                r[1],
                (r[2] - 16.8 * self.scale).max(0.),
                r[3],
            ];
            labels.push(json!({"text":name,"rect":r,"size":16.8*self.scale,"fit":true,"selected":i==category,"color":[59,52,46]}));
        }
        for r in self.imports {
            if r[2]>0. && r[3]>0. {category_hover.push(browser_face(inset(r,2.*self.scale,2.*self.scale),6.*self.scale,if pointer.is_some_and(|p|inside(r,p)){0.76}else{0.28}));}
        }
        if self.search_focused && self.search[2]>0. {category_hover.push(browser_face(self.search,4.*self.scale,0.28));}
        let mut content_labels = Vec::new();
        let mut content_hover = Vec::new();
        let mut content_selected = Vec::new();
        for r in &self.rows {
            if let Some(item) = &r.item {
                let row = intersection(r.rect, self.content);
                if row[2] > 14.4 * self.scale && row[3] > 7.2 * self.scale {
                    let face = inset(row, 7.2 * self.scale, 3.6 * self.scale);
                    if pointer.is_some_and(|p| inside(row, p)) {
                        content_hover.push(browser_face(face, 9.6 * self.scale, 0.28));
                    }
                    if Some(item) == selected {
                        content_selected.push(browser_face(face, 9.6 * self.scale, 0.76));
                    }
                }
            }
            if r.rect[1] < self.content[1]
                || r.rect[1] + r.rect[3] > self.content[1] + self.content[3]
            {
                continue;
            }
            let inset = if r.header { 9.6 } else { 40.8 } * self.scale;
            let rect = [
                r.rect[0] + inset,
                r.rect[1],
                (r.rect[2] - inset - 9.6 * self.scale).max(0.),
                r.rect[3],
            ];
            if r.item.is_some() {
                let icon = [
                    r.rect[0] + 7.2 * self.scale,
                    r.rect[1] + (r.rect[3] - 26.4 * self.scale) * 0.5,
                    26.4 * self.scale,
                    26.4 * self.scale,
                ];
                if pointer.is_some_and(|p| inside(icon, p)) {
                    content_hover.push(browser_face(icon, 6. * self.scale, 0.28));
                }
            }
            content_labels.push(json!({"text":r.text,"rect":rect,"icon":if r.header{Value::Null}else{json!([r.rect[0]+9.6*self.scale,r.rect[1]+(r.rect[3]-21.6*self.scale)*0.5,21.6*self.scale,21.6*self.scale])},"size":if r.header{14.4*self.scale}else{16.8*self.scale},"fit":true,"color":[59,52,46]}));
        }
        category_hover.extend(category_selected);
        content_hover.extend(content_selected);
        json!([{"clip":self.clip,"labels":labels,"browser_faces":category_hover},
               {"clip":self.content,"labels":content_labels,"browser_faces":content_hover}])
    }
}

fn inset(r: [f64; 4], x: f64, y: f64) -> [f64; 4] {
    [
        r[0] + x,
        r[1] + y,
        (r[2] - 2. * x).max(0.),
        (r[3] - 2. * y).max(0.),
    ]
}
fn intersection(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let x = a[0].max(b[0]);
    let y = a[1].max(b[1]);
    [
        x,
        y,
        (a[0] + a[2]).min(b[0] + b[2]).max(x) - x,
        (a[1] + a[3]).min(b[1] + b[3]).max(y) - y,
    ]
}
fn browser_face(rect: [f64; 4], radius: f64, alpha: f64) -> Value {
    // DawIOSystem's original highlight_text default. Native alpha compositing
    // leaves the approved underlying gem facets intact.
    json!({"rect":rect,"radius":radius,"color":[51.,132.6,209.1],"alpha":alpha})
}
const DROP_SPARKLE_SECONDS: f64 = 0.55;
const DROP_SPARKLE_DIRECTIONS: [[f64; 2]; 8] = [
    [1., 0.],
    [0.71, 0.71],
    [0., 1.],
    [-0.71, 0.71],
    [-1., 0.],
    [-0.71, -0.71],
    [0., -1.],
    [0.71, -0.71],
];

#[derive(Default)]
pub(crate) struct Ui {
    pub enabled:bool,
    enable_left:bool,
    enable_pressed:bool,
    pub category: usize,
    pub query:String,
    pub search_active:bool,
    search_left:bool,
    footer_press:Option<usize>,
    pub scroll: f64,
    selected: Option<Item>,
    drag: Option<(Value, u64, [f64; 4])>,
    drag_wave: Value,
    wave_worker: Option<(std::sync::mpsc::Receiver<Value>,std::sync::Arc<std::sync::atomic::AtomicBool>)>,
    previous_left: bool,
    audio_press:Option<[f64;2]>,
    audio_moved:bool,
    captured: bool,
    category_scroll: [f64; 5],
    drop_sparkle: Option<([f64; 2], f64)>,
    pub scanned: bool,
    pub library: super::audio_library::Library,
    pub error: Option<String>,
}
impl Ui {
    /// A complete click authorizes discovery for this application session.
    /// Preserve fast down/up pairs delivered together by the native callback.
    pub(crate) fn enable_input(&mut self,input:&Value,layout:&Layout,blocked:bool)->bool {
        if self.enabled{return false;}
        if blocked || input["focused"]==false || input["transport_pointer_cancelled"]==true {
            self.enable_pressed=false;self.enable_left=input["left"]==true;return false;
        }
        let mut samples=Vec::new();
        for e in input["transport_pointer_events"].as_array().into_iter().flatten().take(16) {
            if let (Some(left),Ok(p))=(e["left"].as_bool(),serde_json::from_value::<[f64;2]>(e["pointer"].clone())) {samples.push((left,p));}
        }
        if let Ok(p)=serde_json::from_value::<[f64;2]>(input["pointer"].clone()){samples.push((input["left"]==true,p));}
        for (left,p) in samples {
            let hit=inside(layout.screen,p)&&inside(layout.clip,p)&&inside(layout.enable_button(),p);
            if left&&!self.enable_left {self.enable_pressed=hit;}
            if !left&&self.enable_left {
                let enable=self.enable_pressed&&hit;self.enable_pressed=false;
                if enable {self.enabled=true;self.enable_left=false;return true;}
            }
            self.enable_left=left;
        }
        false
    }
    pub(crate) fn search_input(&mut self,input:&Value,layout:&Layout,blocked:bool)->bool {
        let left=input["left"]==true;
        let pressed=left&&!self.search_left || input["transport_pointer_events"].as_array().is_some_and(|events|events.iter().any(|e|e["left"]==true));
        self.search_left=left;
        if blocked || input["focused"]==false {self.search_active=false;return false;}
        if pressed {self.search_active=input["pointer"].as_array().and_then(|p|Some([p.first()?.as_f64()?,p.get(1)?.as_f64()?])).is_some_and(|p|inside(layout.search,p)&&inside(layout.screen,p));}
        if !self.search_active{return false;}
        let previous_query=self.query.clone();
        for event in input["browser_keys"].as_array().into_iter().flatten(){
            let key=event[0].as_i64().unwrap_or(0);let mods=event[1].as_i64().unwrap_or(0);
            if key==256 {self.query.clear();}
            else if key==259 {self.query.pop();}
            else if mods&14==0 && self.query.len()<128 {
                let ch=match key {65..=90=>char::from_u32(key as u32+32),51 if mods&1!=0=>Some('#'),48..=57=>char::from_u32(key as u32),32=>Some(' '),45=>Some('-'),61=>Some(if mods&1!=0{'+'}else{'='}),47=>Some('/'),44=>Some(','),46=>Some('.'),_=>None};
                if let Some(ch)=ch{self.query.push(ch);}
            }
        }
        if self.query!=previous_query{self.scroll=0.;}true
    }
    pub(crate) fn dragging_audio(&self) -> bool {
        self.drag.as_ref().is_some_and(|(entry, _, _)| entry["format"] == "WAV")
    }
    pub(crate) fn dragging_stock(&self) -> bool {
        self.drag
            .as_ref()
            .is_some_and(|(entry, _, _)| entry["stock"] == "LShaping")
    }
    pub(crate) fn owns_pointer(&self) -> bool {
        self.captured
    }
    pub(crate) fn selected(&self) -> Option<&Item> {
        self.selected.as_ref()
    }
    pub(crate) fn cancel(&mut self) {
        self.cancel_wave();self.drag_wave=Value::Null;self.drag = None;self.audio_press=None;self.footer_press=None;
        self.captured = false;
        self.drop_sparkle = None;
    }
    /// A command proposal is not success. Call only after the host accepts add.
    pub(crate) fn acknowledge_drop(&mut self, point: Option<[f64; 2]>) {
        self.drop_sparkle = point
            .filter(|p| p.iter().all(|n| n.is_finite()))
            .map(|p| (p, 0.));
    }
    pub(crate) fn advance_feedback(&mut self, dt: f64) {
        if let Some((_, age)) = &mut self.drop_sparkle {
            if dt.is_finite() {
                *age += dt.max(0.);
            }
            if *age >= DROP_SPARKLE_SECONDS {
                self.drop_sparkle = None;
            }
        }
    }
    /// Vst3BrowserSystem: eight alternating * / + glyphs, eased expansion,
    /// shrinking 22-to-10 font size, gone after 0.55s; no invented particles.
    pub(crate) fn drop_feedback(&self, screen: [f64; 4]) -> Value {
        let Some((position, age)) = self.drop_sparkle else {
            return Value::Null;
        };
        let t = (age / DROP_SPARKLE_SECONDS).clamp(0., 1.);
        let radius = 7. + 28. * (1. - (1. - t) * (1. - t));
        let size = 10. + 12. * (1. - t);
        let labels:Vec<_>=DROP_SPARKLE_DIRECTIONS.iter().enumerate().map(|(i,d)|{
            let x=position[0]+d[0]*radius;let y=position[1]+d[1]*radius;
            json!({"text":if i%2==0{"*"}else{"+"},"rect":[x-size,y-size,size*2.,size*2.],"size":size,"center":true,"color":[244,241,232]})
        }).collect();
        json!({"clip":screen,"labels":labels})
    }
    pub(crate) fn component_hit(&self, layout: &Layout, p: [f64; 2]) -> Option<browser::Action> {
        match layout.hit(p) {
            Some(Item::Component(k)) => Some(browser::Action::NewTrack(k)),
            _ => None,
        }
    }
    fn cancel_wave(&mut self){if let Some((_,cancel))=self.wave_worker.take(){cancel.store(true,std::sync::atomic::Ordering::Relaxed);}}
    pub(crate) fn ghost(&self, p: Option<[f64; 2]>) -> Value {
        let Some((entry, _, _)) = &self.drag else {
            return Value::Null;
        };
        let Some(p) = p else {
            return Value::Null;
        };
        let text = super::plugin_surface::label_text(entry["name"].as_str().unwrap_or(""), 64);
        json!({"text":text,"position":[p[0]+12.,p[1]+8.],
            "preview":if entry["format"]=="WAV"{"tape"}else if entry["stock"]=="LShaping"{"eq"}else{"plugin"},
            "wave":self.drag_wave,
            "eq":if entry["stock"]=="LShaping"{super::stock_eq::presentation(&json!({"state":{"gain_db":0,"edges":[{"frequency_hz":180,"slope_db_oct":12},{"frequency_hz":6000,"slope_db_oct":12}]},"bypassed":false,"response_db":vec![0.;97]}),[0.,36.,230.,132.],1.)}else{Value::Null}})
    }
    /// Drop is an existing document track, never a component insertion boundary.
    pub(crate) fn update(
        &mut self,
        input: &Input,
        layout: &Layout,
        revision: u64,
        canvas: [f64; 4],
        drop: Option<&Value>,
        blocked: bool,
    ) -> Option<Value> {
        if let Some((rx,_))=&self.wave_worker {if let Ok(wave)=rx.try_recv(){self.drag_wave=wave;self.wave_worker=None;}}
        let pressed = input.left && !self.previous_left;
        let released = !input.left && self.previous_left;
        self.previous_left = input.left;
        if blocked
            || input.escape
            || input.pointer.is_none()
            || self
                .drag
                .as_ref()
                .is_some_and(|(_, r, c)| *r != revision || *c != canvas)
        {
            self.cancel_wave();self.drag = None;self.footer_press=None;
            self.captured &= input.left;
            return None;
        }
        let p = input.pointer.unwrap();
        if let Some(origin)=self.audio_press{self.audio_moved|=(p[0]-origin[0]).abs()>3.||(p[1]-origin[1]).abs()>3.;}
        // Cardinal consumes category-column wheel separately. These three
        // supported categories fit without moving the contents column.
        if inside(layout.screen, p)
            && inside(layout.content, p)
            && input.wheel.iter().all(|n| n.is_finite())
        {
            self.scroll =
                (self.scroll + input.wheel[1] * 43.2 * layout.scale).clamp(layout.minimum, 0.);
        }
        if pressed {
            if inside(layout.back,p)&&inside(layout.screen,p){self.category=4;self.scroll=0.;self.captured=true;return None;}
            if let Some(i)=layout.imports.iter().position(|r|inside(*r,p)&&inside(layout.screen,p)) {
                self.captured=true;self.footer_press=Some(i);return None;
            }
            if inside(layout.search,p){self.captured=true;return None;}
            if let Some(i) = layout
                .categories
                .iter()
                .position(|r| inside(*r, p) && inside(layout.clip, p) && inside(layout.screen, p))
            {
                self.category_scroll[self.category] = self.scroll;
                self.category = i;
                self.scroll = self.category_scroll[i];
                self.captured = true;
            } else if let Some(item) = layout.hit(p) {
                self.selected = Some(item.clone());
                if let Item::Plugin(entry) | Item::Audio(entry) = item {
                    self.captured = true;
                    // Decode once on the control side, never while moving or in audio callbacks.
                    self.cancel_wave();self.drag_wave=Value::Null;
                    if entry["format"]=="WAV" {if let Some(path)=entry["path"].as_str(){
                        let path=std::path::PathBuf::from(path);let (tx,rx)=std::sync::mpsc::channel();
                        let cancel=std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));let worker_cancel=cancel.clone();
                        std::thread::spawn(move||{let wave=super::audio_library::drag_wave(&path,&worker_cancel).unwrap_or(Value::Null);let _=tx.send(wave);});
                        self.wave_worker=Some((rx,cancel));
                    }}
                    self.audio_press=(entry["format"]=="WAV").then_some(p);self.audio_moved=false;
                    self.drag = Some((entry, revision, canvas));
                }
            }
        }
        if released {
            self.captured = false;
            if let Some(i)=self.footer_press.take(){return inside(layout.imports[i],p).then(||json!({"op":if i==0{"choose_audio"}else{"choose_plugins"}}));}
            self.cancel_wave();let (entry, _, _) = self.drag.take()?;
            let clicked=self.audio_press.take().is_some()&&!self.audio_moved&&matches!(layout.hit(p),Some(Item::Audio(ref e))if e["path"]==entry["path"]);
            if clicked{return Some(json!({"op":"preview_audio","path":entry["path"]}));}
            let track = drop?;
            if track["kind"] != "audio" && track["kind"] != "midi" && track["kind"] != "mix" && track["kind"] != "channel" {
                return None;
            }
            if entry["instrument"] == true && track["kind"] != "midi" {
                return None;
            }
            if entry["format"] == "WAV" {
                if track["kind"] != "audio" { return None; }
                return Some(json!({"op":"import_audio","track":track["id"],"path":entry["path"]}));
            }
            let mut command = json!({"op":"add","track":track["id"]});
            if entry["stock"] == "LShaping" {
                command["stock"] = entry["stock"].clone();
            } else {
                command["index"] = entry["index"].clone();
            }
            return Some(command);
        }
        None
    }
}

#[cfg(test)]
#[path = "PluginBrowserFeedbackTests.rs"]
mod feedback_tests;

impl Drop for Ui{fn drop(&mut self){self.cancel_wave();}}

#[cfg(test)]mod access_tests {
 use super::*;
 fn gate()->Layout {let mut motion=super::super::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);layout(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default(),&[],0,0.).unwrap()}
 #[test]fn browser_enable_requires_a_complete_inside_click_and_respects_blocking(){
  let l=gate();let r=l.enable_button();let p=[r[0]+r[2]*0.5,r[1]+r[3]*0.5];let mut ui=Ui::default();
  assert!(!ui.enabled);
  assert!(!ui.enable_input(&json!({"pointer":p,"left":true}),&l,false));
  assert!(!ui.enable_input(&json!({"pointer":[0.,0.],"left":false}),&l,false));assert!(!ui.enabled);
  assert!(!ui.enable_input(&json!({"pointer":p,"left":true}),&l,true));
  assert!(!ui.enable_input(&json!({"pointer":p,"left":false}),&l,false));assert!(!ui.enabled);
  assert!(!ui.enable_input(&json!({"pointer":p,"left":true}),&l,false));
  assert!(ui.enable_input(&json!({"pointer":p,"left":false}),&l,false));assert!(ui.enabled);
  assert!(!ui.enable_input(&json!({"pointer":p,"left":false}),&l,false));
 }
 #[test]fn fast_browser_enable_click_is_retained_and_focus_loss_cancels(){
  let l=gate();let r=l.enable_button();let p=[r[0]+r[2]*0.5,r[1]+r[3]*0.5];let event=json!({"pointer":p,"left":false,"transport_pointer_events":[{"pointer":p,"left":true},{"pointer":p,"left":false}]});
  let mut ui=Ui::default();let mut cancelled=event.clone();cancelled["transport_pointer_cancelled"]=json!(true);
  assert!(!ui.enable_input(&cancelled,&l,false));assert!(!ui.enabled);
  assert!(ui.enable_input(&event,&l,false));
  let labels=l.locked_presentation();assert_eq!(labels[0]["labels"][1]["text"],"ENABLE BROWSER");assert!(super::super::plugin_surface::valid(&json!({"groups":labels})));
 }
}

#[cfg(test)]mod artwork_drag_tests{
 use super::*;
 #[test]fn artwork_packets_preserve_payload_and_pass_native_validation(){
  for (entry,kind) in [(json!({"stock":"LShaping","name":"U Shaping EQ"}),"eq"),(json!({"format":"WAV","name":"sample.wav"}),"tape"),(json!({"name":"Effect"}),"plugin")]{
   let mut ui=Ui::default();ui.drag=Some((entry,1,[0.,0.,800.,600.]));
   let ghost=ui.ghost(Some([100.,200.]));assert_eq!(ghost["preview"],kind);
   if kind=="eq"{assert!(super::super::plugin_surface::valid(&json!({"groups":[{"clip":[0.,0.,300.,220.],"graphs":[ghost["eq"].clone()],"labels":[]}]})));}
   ui.cancel();assert!(ui.ghost(Some([100.,200.])).is_null());
  }
 }
}
