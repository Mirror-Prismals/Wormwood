//! Cardinal's saved 2D appearance data, independent of audio/document state.
//! The 3D gem ruby is still Wormwood's main theme and the default. Cardinal
//! presets are explicit alternatives; importing one never changes that default.
//! All loading/editing runs on the control thread, never an audio callback.
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock}};
pub(crate) type Color = [f32;4];
const LIMIT: u64 = 1024*1024;
const DEFAULT: &str = include_str!("../Art/AudioWorkstation/CardinalThemes/Default.json");
#[derive(Clone, Debug)]
pub(crate) struct Theme { pub name:String, pub document:Value, colors:BTreeMap<String,Color> }
#[derive(Clone,Debug)]
pub(crate) struct Material { pub color:Color, pub a:Color, pub b:Color, pub gradient:[f32;4], pub effects:[f32;4], pub glyph:Color, pub gem:bool, pub jelly:bool }
pub(crate) fn color_hex(s:&str)->Result<Color,String>{
 let s=s.strip_prefix('#').unwrap_or(s);
 if s.len()!=8||!s.is_ascii(){return Err("theme colors require RRGGBBAA (not ARGB)".into());}
 let mut c=[0.;4];for(i,x)in c.iter_mut().enumerate(){*x=u8::from_str_radix(&s[i*2..i*2+2],16).map_err(|_|"invalid theme hex color")? as f32/255.;}Ok(c)
}
impl Theme {
 pub fn load(path:&Path)->Result<Self,String>{
  let file=std::fs::File::open(path).map_err(|e|format!("{}: {e}",path.display()))?;
  if file.metadata().map_err(|e|e.to_string())?.len()>LIMIT{return Err("theme exceeds 1 MiB".into());}
  use std::io::Read;let mut bytes=Vec::new();file.take(LIMIT+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
  if bytes.len()as u64>LIMIT{return Err("theme exceeds 1 MiB".into());}
  Self::parse(serde_json::from_slice(&bytes).map_err(|e|format!("theme JSON: {e}"))?)
 }
 pub fn parse(document:Value)->Result<Self,String>{
  if document["format"]!="salamander_theme_preset"||!matches!(document["version"].as_u64(),Some(1..=5)){return Err("expected salamander_theme_preset version 1..5".into());}
  let name=document["name"].as_str().filter(|s|!s.is_empty()&&s.len()<=256).ok_or("theme needs a name (1..256 bytes)")?.to_owned();
  for field in ["emissive_intensity","background_grain"]{if let Some(v)=document.get(field){finite_unit(v,field)?;}}
  let mut colors=BTreeMap::new();
  for field in ["panel","panel_top","panel_bottom","panel_left","panel_right","button","lane","background","piano_roll","piano_roll_accent","transport_rewind_button","transport_stop_button","transport_record_button","transport_play_button","transport_rewind_glyph","transport_stop_glyph","transport_record_glyph","transport_play_glyph"] {
   if let Some(v)=document.get(field){colors.insert(field.into(),color_hex(v.as_str().ok_or_else(||format!("{field} must be a hex color"))?)?);}
  }
  for field in ["button_colors","glyph_colors","surface_colors","text_colors"] {
   if let Some(v)=document.get(field){for(k,c)in v.as_object().ok_or_else(||format!("{field} must be an object"))?{colors.insert(format!("{field}:{k}"),color_hex(c.as_str().ok_or("theme map values must be hex colors")?)?);}}
  }
  if let Some(v)=document.get("gradients") {for(_,g)in v.as_object().ok_or("gradients must be an object")?{
   for key in ["color_a","color_b"]{color_hex(g[key].as_str().ok_or("gradient requires both colors")?)?;}
   for key in ["anchor_a","anchor_b"]{if !matches!(g[key].as_u64(),Some(0..=4)){return Err("gradient anchor must be 0..4".into());}}
   finite_unit(&g["warp"],"gradient warp")?;
  }}
  for field in ["element_emissive","element_gloss","element_grain","element_specular"]{
   if let Some(v)=document.get(field){for(_,x)in v.as_object().ok_or("material effects must be objects")?{finite_unit(x,field)?;}}
  }
  for(field,n)in [("midi_pitch_palette",12),("waveform_fft_palette",5)]{if let Some(v)=document.get(field){let a=v.as_array().filter(|a|a.len()==n).ok_or_else(||format!("{field} needs {n} colors"))?;for c in a{color_hex(c.as_str().ok_or("palette color must be hex")?)?;}}}
  if let Some(v)=document.get("meter_palettes"){let a=v.as_array().filter(|a|a.len()==3).ok_or("meter_palettes needs three palettes")?;for palette in a{let p=palette.as_array().filter(|a|a.len()==6).ok_or("meter palette needs six colors")?;for c in p{color_hex(c.as_str().ok_or("meter color must be hex")?)?;}}}
  Ok(Self{name,document,colors})
 }
 pub fn color(&self,key:&str,fallback:Color)->Color{self.colors.get(key).copied().unwrap_or(fallback)}
 pub fn surface(&self,key:&str,fallback:Color)->Color{self.color(&format!("surface_colors:{key}"),fallback)}
 pub fn palette(&self,field:&str,index:usize,fallback:Color)->Color{self.document[field].get(index).and_then(Value::as_str).and_then(|s|color_hex(s).ok()).unwrap_or(fallback)}
 pub fn meter(&self,family:usize,index:usize)->Color{self.document["meter_palettes"][family%3][index.min(5)].as_str().and_then(|s|color_hex(s).ok()).unwrap_or([1.;4])}
 pub fn material(&self,kind:&str,id:&str)->Material{
  let ivory=[230./255.,224./255.,212./255.,1.];let generic=self.color(if kind=="panel"{"panel"}else{"button"},ivory);
  let mut color=match kind {
   "panel"=>self.color(id,generic),
   "button"=>self.color(&format!("button_colors:{id}"),self.color(&format!("{id}_button"),generic)),
   "glyph"=>self.color(&format!("glyph_colors:{id}"),self.color(&format!("{id}_glyph"),[17./255.,17./255.,17./255.,1.])),
   "text"=>self.color(&format!("text_colors:{id}"),self.surface("button_text",self.surface("text",[0.12,0.14,0.14,1.]))),
   _=>{
    let default=if id.starts_with("clip:"){"clip_default:midi"}else if id.starts_with("track:"){"track_default:audio"}else if id.starts_with("fader_knob:"){"fader_knob_default"}else if id.starts_with("fader_housing:"){"fader_housing_default"}else{id};
    self.surface(id,self.surface(default,if id.starts_with("clip:"){self.color("lane",ivory)}else{generic}))
   }
  };
  let key=format!("{kind}:{id}");let g=&self.document["gradients"][&key];let on=g["enabled"].as_bool().unwrap_or(false);
  let a=g["color_a"].as_str().and_then(|s|color_hex(s).ok()).unwrap_or(color);let b=g["color_b"].as_str().and_then(|s|color_hex(s).ok()).unwrap_or(color);
  if on{color[3]=(a[3]+b[3])*0.5;}
  let effects=["element_emissive","element_grain","element_specular","element_gloss"].map(|field|self.document[field][&key].as_f64().unwrap_or(if field=="element_emissive"{self.document["emissive_intensity"].as_f64().unwrap_or(0.)}else{0.})as f32);
  let field=match kind{"panel"=>"panel_material","button"=>"button_material",_ if id.starts_with("fader_")=>"fader_material",_ if id.starts_with("track_")=>"track_handle_material",_=>"timeline_material"};
  Material{color,a,b,gradient:[if on{1.}else{0.},g["anchor_a"].as_u64().unwrap_or(0)as f32,g["anchor_b"].as_u64().unwrap_or(3)as f32,g["warp"].as_f64().unwrap_or(0.35)as f32],effects,glyph:self.material_glyph(id),gem:kind!="text"&&kind!="glyph"&&id!="midi_note"&&!id.starts_with("meter:")&&self.document[field]=="gem",jelly:kind!="text"&&kind!="glyph"&&self.document[field]=="jelly"}
 }
 fn material_glyph(&self,id:&str)->Color{self.color(&format!("glyph_colors:{id}"),self.color(&format!("{id}_glyph"),[17./255.,17./255.,17./255.,1.]))}
 /// Save a copy without discarding fields unsupported by the current host.
 /// Callers choose the destination explicitly; bundled presets stay immutable.
 pub fn save_copy(&self,path:&Path)->Result<(),String>{let mut d=self.document.clone();d["builtin"]=json!(false);std::fs::write(path,serde_json::to_vec_pretty(&d).map_err(|e|e.to_string())?).map_err(|e|e.to_string())}
 pub fn matches_artwork(&self,source:&Value)->bool{let mut expected=self.document.clone();let mut source=source.clone();if let Some(d)=expected.as_object_mut(){d.remove("builtin");}if let Some(d)=source.as_object_mut(){d.remove("builtin");}artwork_equal(&expected,&source)}
 pub fn summary(&self)->Value{json!({"name":self.name,"format":"salamander_theme_preset","version":self.document["version"],"renderer":"cardinal-2d","main_theme":"3D gem ruby","fields_preserved":true})}
}
// Python's offline JSON writer and serde_json can decode the same decimal
// one ULP apart. Ignore only numerical roundoff, never changed color strings,
// keys or meaningful material settings, when checking immutable artwork.
fn artwork_equal(a:&Value,b:&Value)->bool{
 if a==b{return true;}
 match(a,b){
  (Value::Number(a),Value::Number(b))=>match(a.as_f64(),b.as_f64()){(Some(a),Some(b))=>(a-b).abs()<=1e-12*(1.+a.abs().max(b.abs())),_=>false},
  (Value::Array(a),Value::Array(b))=>a.len()==b.len()&&a.iter().zip(b).all(|(a,b)|artwork_equal(a,b)),
  (Value::Object(a),Value::Object(b))=>a.len()==b.len()&&a.iter().all(|(k,v)|b.get(k).is_some_and(|b|artwork_equal(v,b))),_=>false
 }
}
fn finite_unit(v:&Value,name:&str)->Result<(),String>{if !v.as_f64().is_some_and(|n|n.is_finite()&&(0. ..=1.).contains(&n)){return Err(format!("{name} must be a finite number in 0..1"));}Ok(())}
static GENERATION:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
static TOKEN:OnceLock<Mutex<Option<String>>>=OnceLock::new();
fn token()->&'static Mutex<Option<String>>{TOKEN.get_or_init(||Mutex::new(None))}
static OVERRIDE:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
pub fn generation()->u64{GENERATION.load(std::sync::atomic::Ordering::Relaxed)}
pub fn label(root:&Path)->String{let selected=selected(root).ok().flatten();token().lock().ok().and_then(|v|v.clone()).unwrap_or_else(||selected.map_or_else(||"ruby-3d".into(),|t|t.name.clone()))}
pub fn select_preference(root:&Path,name:&str)->Result<(),String>{if OVERRIDE.load(std::sync::atomic::Ordering::Relaxed)||std::env::var_os("WORMWOOD_THEME").is_some(){return Ok(());}set_selection(root,name)}
static CHOICE:OnceLock<Mutex<Option<Option<Arc<Theme>>>>>=OnceLock::new();
fn choice()->&'static Mutex<Option<Option<Arc<Theme>>>>{CHOICE.get_or_init(||Mutex::new(None))}
pub fn select(root:&Path,name:&str)->Result<(),String>{OVERRIDE.store(true,std::sync::atomic::Ordering::Relaxed);set_selection(root,name)}
pub fn validate_selection(root:&Path,name:&str)->Result<(),String>{
 if name=="ruby-3d"{return Ok(());}
 let theme=load_choice(root,name)?;
 if theme.document.as_object().is_some_and(|d|d.iter().any(|(k,v)|k.ends_with("_material")&&v=="gem")) {
  if theme.name.contains('/')||theme.name.contains('\\')||theme.name==".."{return Err("invalid crystal atlas name".into());}
  let p=root.join("Art/AudioWorkstation/CardinalThemes/Baked").join(&theme.name).join("manifest.json");
  let d:Value=serde_json::from_slice(&std::fs::read(&p).map_err(|_|"Crystal theme needs an explicit offline bake_cardinal_themes.py build")?).map_err(|e|e.to_string())?;
  if !theme.matches_artwork(&d["theme"]){return Err("Crystal theme artwork is stale; rebuild with bake_cardinal_themes.py".into());}
 }
 Ok(())
}
fn set_selection(root:&Path,name:&str)->Result<(),String>{
 validate_selection(root,name)?;
 let theme=if name=="ruby-3d"{None}else{Some(Arc::new(load_choice(root,name)?))};
 let mut current=choice().lock().map_err(|_|"theme selection lock poisoned")?;
 if current.as_ref().is_some_and(|old|match(old,&theme){(None,None)=>true,(Some(a),Some(b))=>a.document==b.document,_=>false}){return Ok(());}
 *token().lock().map_err(|_|"theme token lock poisoned")?=Some(name.to_owned());
 *current=Some(theme);GENERATION.fetch_add(1,std::sync::atomic::Ordering::Relaxed);Ok(())
}
fn load_choice(root:&Path,name:&str)->Result<Theme,String>{
 if name=="cardinal"{return Theme::parse(serde_json::from_str(DEFAULT).map_err(|e|e.to_string())?);}
 let path=PathBuf::from(name);if path.is_file(){return Theme::load(&path);}
 if name.contains('/')||name.contains('\\')||name==".."{return Err("theme file not found".into());}
 Theme::load(&root.join("Art/AudioWorkstation/CardinalThemes").join(format!("{}.json",name.strip_suffix(".json").unwrap_or(name))))
}
pub fn root()->PathBuf{std::env::var_os("WORMWOOD_ASSET_ROOT").map(PathBuf::from).unwrap_or_else(||PathBuf::from(env!("CARGO_MANIFEST_DIR")))}
pub fn presets(root:&Path)->Vec<String>{let mut names=vec!["ruby-3d".into()];if let Ok(entries)=std::fs::read_dir(root.join("Art/AudioWorkstation/CardinalThemes")){let mut saved:Vec<String>=entries.flatten().filter_map(|e|{let p=e.path();(p.extension().is_some_and(|s|s=="json")).then(||p.file_stem().unwrap().to_string_lossy().into_owned())}).collect();saved.sort();names.extend(saved);}names}
pub fn selected(root:&Path)->Result<Option<Arc<Theme>>,String>{
 let mut c=choice().lock().map_err(|_|"theme selection lock poisoned")?;
 if c.is_none(){*c=Some(match std::env::var("WORMWOOD_THEME").ok().filter(|s|!s.is_empty()&&s!="ruby-3d"){Some(n)=>{validate_selection(root,&n)?;*token().lock().map_err(|_|"theme token lock poisoned")?=Some(n.clone());Some(Arc::new(load_choice(root,&n)?))},None=>None});}
 Ok(c.as_ref().cloned().flatten())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn cardinal_presets_load_without_losing_fields(){let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("Art/AudioWorkstation/CardinalThemes");for f in std::fs::read_dir(root).unwrap(){let p=f.unwrap().path();if p.extension().is_some_and(|s|s=="json"){let raw:Value=serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();let t=Theme::load(&p).unwrap_or_else(|e|panic!("{}: {e}",p.display()));assert_eq!(raw,t.document);}}}
 #[test]fn rgba_and_target_fallbacks_match_cardinal(){assert_eq!(color_hex("FF0000D6").unwrap(),[1.,0.,0.,214./255.]);let mut d:Value=serde_json::from_str(DEFAULT).unwrap();d["surface_colors"]["fader_knob_default"]=json!("12345678");let t=Theme::parse(d).unwrap();assert_eq!(t.material("surface","fader_knob:audio:1").color,color_hex("12345678").unwrap());assert_eq!(t.material("panel","panel_left").color,color_hex("E6E0D4CC").unwrap());}
 #[test]fn rejects_invalid_effects_and_bad_color(){let mut d:Value=serde_json::from_str(DEFAULT).unwrap();d["element_grain"]["panel:panel_top"]=json!(2.);assert!(Theme::parse(d).is_err());assert!(color_hex("00FF00").is_err());assert!(color_hex("é000000").is_err());}
}
