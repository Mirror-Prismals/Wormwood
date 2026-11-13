//! Matching Blender mesh presentation; first-person worn optics remain passthrough.
use crate::{audio::*,host::Host,base_system::layer_five::glasses_system::{Carry,presented}};
use serde_json::{Value,json};
use std::{collections::BTreeMap,sync::Arc};
#[derive(Default)]pub(crate) struct State {pub models:BTreeMap<String,Arc<Vec<[f32;6]>>>}
pub(crate) fn frame(host:&Host,view:&Value)->Result<Vec<[f32;24]>,String>{
    let eye=vector(&view["position"])?;let mut vertices=Vec::new();
    for pair in &host.glasses.pairs {
        if pair.location==Carry::Worn && length(sub(pair.pose.position,eye))<0.3 {continue;}
        let pose=if pair.location==Carry::Worn{pair.pose}else{presented(pair,view)?};if length(sub(pose.position,eye))>45.{continue;}
        let Some(mesh)=host.glasses_render.models.get(pair.kind)else{continue;};
        for t in mesh.chunks_exact(3){let points:[V;3]=std::array::from_fn(|i|add(sub(pose.position,eye),add(mul(pose.right,t[i][0]as f64),add(mul(pose.up,t[i][1]as f64),mul(pose.front,t[i][2]as f64)))));
            let normal=unit(cross(sub(points[1],points[0]),sub(points[2],points[0])));let shade=(0.45+0.55*dot(normal,unit(mul(points[0],-1.))).abs())as f32;
            for i in 0..3{vertices.push([points[i][0]as f32,points[i][1]as f32,points[i][2]as f32,t[i][3]*shade,t[i][4]*shade,t[i][5]*shade]);}
        }
    }
    let mut local=view.clone();local["position"]=json!([0.,0.,0.]);local["two_sided"]=json!(true);
    Ok(super::appearance_system::projection::project(&vertices,&local))
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{match hook {
    "Load"=>{let bytes=std::fs::read(host.root().join("Procedures/Assets/Glasses/glasses_mesh.json")).map_err(|e|format!("glasses model: {e}"))?;
        if bytes.len()>8*1024*1024{return Err("glasses model exceeds byte budget".into());}let data:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
        if data["schema_version"]!=1{return Err("unsupported glasses model".into());}
        let mut models=BTreeMap::new();for kind in ["ar","vr"]{let v:Vec<[f32;6]>=serde_json::from_value(data["models"][kind]["vertices"].clone()).map_err(|e|e.to_string())?;
            if v.is_empty()||v.len()%3!=0||v.len()>18000||v.iter().flatten().any(|n|!n.is_finite()){return Err("invalid glasses mesh".into());}models.insert(kind.into(),Arc::new(v));}
        host.glasses_render.models=models;Ok(Value::Null)},
    "Frame"=>Ok(json!({"triangles":frame(host,input)?})),_=>Err("unknown glasses render hook".into())
}}
