//! Compatibility facade for the loaded mod's radial realm provider.
//! No dimension layout, palette, terrain formula, or portal location lives here.
use crate::audio::V;
pub(crate) fn enabled()->bool{crate::modding::provides("nightworld")}
pub(crate) fn active(p:V)->bool{enabled()&&crate::modding::query(0,[p[0],p[1],p[2],0.]).is_some_and(|v|v[0]>0.)}
pub(crate) fn world(p:V)->V{let mut p=super::underground::world(p);p[1]= -p[1];p}
pub(crate) fn local(p:V)->V{super::underground::local(p)}
pub(crate) fn height(p:V)->f64{crate::modding::query(1,[p[0],p[1],p[2],0.]).map_or(0.,|v|v[0])}
pub(crate) fn distance(p:V,h:f64)->f64{crate::modding::query(2,[p[0],p[1],p[2],h]).map_or(-1.,|v|v[0])}
pub(crate) fn color(p:V)->[f32;3]{crate::modding::query(3,[p[0],p[1],p[2],0.]).map_or([0.;3],|v|[v[0]as f32,v[1]as f32,v[2]as f32])}
pub(crate) fn shaft()->V{crate::modding::query(4,[0.;4]).map_or([0.;3],|v|[v[0],v[1],v[2]])}
pub(crate) fn shaft_radius(p:V)->f64{let c=shaft();(p[0]-c[0]).hypot(p[2]-c[2])}
pub(crate) fn portal_level()->f64{crate::modding::query(5,[0.;4]).map_or(f64::NEG_INFINITY,|v|v[0])}
use crate::audio::unit;
pub(crate) fn mesh(field:&super::underground::Field,center:V,edits:&[serde_json::Value])->Result<Vec<super::underground::Triangle>,String>{
 let q=local(center);let step=1.5;
 let origin=[(q[0]/3.).floor()*3.-21.,(q[1]/3.).floor()*3.-18.,(q[2]/3.).floor()*3.-21.];
 let surface=super::surface_net::extract(origin,[28,24,28],step,|p|{
  let w=world(p);let n=unit(w);field.distance(w,super::tool_cut::height(n,super::height(n),edits,true))
 })?;
 let mut out=Vec::with_capacity(surface.triangles.len());
 for t in surface.triangles {out.push(t.map(|i| {let p=world(surface.vertices[i as usize]);let c=color(p);[p[0],p[1],p[2],c[0] as f64,c[1] as f64,c[2] as f64]}));}
 if out.len()>16000{return Err("Nightworld cave mesh budget".into());}Ok(out)
}
