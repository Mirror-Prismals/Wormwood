//! Offline Cardinal crystal artwork. Selection only loads it; it cannot bake.
use crate::workstation::cardinal_theme::Theme;
use serde_json::Value;
use std::path::Path;
#[derive(Clone)]
struct Entry{key:String,aspect:f32,offset:usize,edge:usize,stride:usize}
pub(super) struct Atlas{entries:Vec<Entry>}
impl Atlas{
 pub fn load(root:&Path,theme:&Theme)->Result<(Self,Vec<u8>),String>{
  let needs=theme.document.as_object().is_some_and(|d|d.iter().any(|(k,v)|k.ends_with("_material")&&v=="gem"));
  if !needs{return Ok((Self{entries:vec![]},vec![0;16]));}
  if theme.name.contains('/')||theme.name.contains('\\')||theme.name==".."{return Err("invalid Cardinal atlas name".into());}
  let folder=root.join("Art/AudioWorkstation/CardinalThemes/Baked").join(&theme.name);
  let manifest:Value=serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).map_err(|e|format!("Cardinal crystal artwork missing: {e}; run the explicit offline bake_cardinal_themes.py tool"))?).map_err(|e|e.to_string())?;
  if manifest["schema"]!=1||!theme.matches_artwork(&manifest["theme"]){return Err("Cardinal artwork does not match this preset; rebuild it with the offline bake_cardinal_themes.py tool".into());}
  let pixels=std::fs::read(folder.join("images.rgba8")).map_err(|e|e.to_string())?;
  if pixels.is_empty()||pixels.len()%4!=0||pixels.len()>64*1024*1024{return Err("invalid Cardinal atlas byte size".into());}
  let mut entries=Vec::new();
  for e in manifest["entries"].as_array().ok_or("Cardinal atlas entries missing")?{
   let n=|key:&str|e[key].as_u64().and_then(|v|usize::try_from(v).ok()).ok_or_else(||format!("invalid Cardinal atlas {key}"));
   let offset=n("offset")?;let edge=n("edge")?;let stride=n("stride")?;let aspect=e["aspect"].as_f64().unwrap_or(0.)as f32;
   if edge==0||edge>2048||stride<edge||!aspect.is_finite()||aspect<=0.||offset.checked_add((edge-1)*stride+edge).is_none_or(|last|last>pixels.len()/4){return Err("invalid Cardinal atlas bounds".into());}
   entries.push(Entry{key:e["key"].as_str().ok_or("Cardinal atlas key missing")?.into(),offset,edge,stride,aspect});
  }
  Ok((Self{entries},pixels))
 }
 pub fn image(&self,key:&str,aspect:f32)->[f32;4]{
  let fallback=if key.starts_with("button:"){"button:*"}else if key.starts_with("surface:fader_knob:"){"surface:fader_knob_default"}else if key.starts_with("surface:fader_housing:"){"surface:fader_housing_default"}else if key.starts_with("surface:track:"){"surface:track_default:audio"}else if key.starts_with("surface:clip:"){"surface:clip_default:midi"}else{"surface:timeline"};
  let wanted=if self.entries.iter().any(|e|e.key==key){key}else{fallback};
  self.entries.iter().filter(|e|e.key==wanted).min_by(|a,b|(a.aspect/aspect).ln().abs().total_cmp(&(b.aspect/aspect).ln().abs())).map(|e|[e.offset as f32,e.edge as f32,e.stride as f32,1.]).unwrap_or([0.;4])
 }
}

#[cfg(test)] mod tests{
 use super::*;
 #[test] fn cardinal_crystal_atlases_match_every_bundled_preset(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR"));
  for entry in std::fs::read_dir(root.join("Art/AudioWorkstation/CardinalThemes")).unwrap(){let p=entry.unwrap().path();if p.extension().is_some_and(|s|s=="json"){
   let theme=Theme::load(&p).unwrap();let(_,pixels)=Atlas::load(root,&theme).unwrap_or_else(|e|panic!("{}: {e}",p.display()));assert!(!pixels.is_empty());
  }}
 }
}
