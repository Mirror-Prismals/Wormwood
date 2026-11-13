//! Native mod rendering adapters; Prismals owns incense/smoke construction.
use crate::{audio::V,host::Host,base_system::layer_five::monastery_system::State};
use serde_json::{json,Value};
pub(crate) fn geometry(state:&State,base:V,lighter:V,view:&Value,now:f64)->Result<Vec<[f32;24]>,String>{
 let value=crate::modding::call_selected("prismals","Prismals.IncenseGeometry",&json!({"state":{"lit":state.lit,"ignition":state.ignition,"spent":state.spent,"lighter":state.lighter},"base":base,"lighter":lighter,"view":view,"now":now}))?;
 serde_json::from_value(value).map_err(|e|e.to_string())
}
pub(crate) fn frame(host:&Host,view:&Value)->Result<Vec<[f32;24]>,String>{
 let input=json!({"input":view,"context":{"origin":host.theatre.origin,"output":host.output_root(),"audicle":host.active_audicle}});
 let value=crate::modding::call_selected("prismals","MonasteryRenderSystem.Frame",&input)?;
 serde_json::from_value(value["triangles"].clone()).map_err(|e|e.to_string())
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{if hook=="Frame"{Ok(json!({"triangles":frame(host,input)?}))}else{Err("unknown mod render hook".into())}}
