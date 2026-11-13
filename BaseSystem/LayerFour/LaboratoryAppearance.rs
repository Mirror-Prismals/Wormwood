//! Native mod appearance adapters; no laboratory rendering behavior lives here.
use crate::{audio::V,base_entity::{Appearance,Entity}};
use serde_json::{json,Value};
pub(crate) fn shade_mouse(vertices:&mut [[f32;6]],instance:&Value,view:&Value)->Result<(),String>{
 let value=crate::modding::call_selected("prismals","Prismals.ShadeMouse",&json!({"vertices":vertices,"instance":instance,"view":view}))?;
 let result:Vec<[f32;6]>=serde_json::from_value(value).map_err(|e|e.to_string())?;
 if result.len()!=vertices.len()||result.iter().flatten().any(|x|!x.is_finite()){return Err("invalid mod appearance vertices".into());}vertices.copy_from_slice(&result);Ok(())
}
pub(crate) fn portal_view(_site:&crate::theatre::laboratory::Site,origin:V,view:&Value,entities:&std::collections::HashMap<u64,std::sync::Arc<Entity>>)->Result<Vec<[f32;24]>,String>{
 if view["laboratory_portals"]!=true{return Ok(Vec::new());}
 let mut selected=std::collections::HashMap::new();
 for item in view["instances"].as_array().into_iter().flatten(){if item["laboratory"]==true{if let Some(id)=item["entity"].as_u64(){if let Some(entity)=entities.get(&id){if let Appearance::Visual{vertices,..}=&entity.appearance{selected.insert(id,vertices);}}}}}
 let result=crate::modding::call_selected("prismals","Prismals.PortalView",&json!({"origin":origin,"view":view,"entities":selected}))?;
 serde_json::from_value(result).map_err(|e|e.to_string())
}
