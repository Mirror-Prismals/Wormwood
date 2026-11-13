//! Authored venue placement and independent rolling furniture ownership.
use crate::{audio::*, host::Host};
use serde_json::{Value,json};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct State {
    pub nightworld: Option<Arc<crate::theatre::nightworld::Site>>,
    pub monitor_image: Option<Vec<u8>>,
    pub origin: V,
    pub chair: Arc<crate::theatre::office_chair::Chair>,
    pub flow: crate::theatre::waterpark::Flow,
    pub venue: Option<Arc<crate::theatre::Venue>>,
}
impl State {
    pub fn world(&self,p:V)->V {add(self.origin,p)}
    pub fn local(&self,p:V)->V {sub(p,self.origin)}
    pub fn computer_position(&self)->Option<V> {self.venue.as_ref().map(|v|self.world(v.computer_position))}
    pub fn microphone_listener(&self)->Option<Value> {
        self.venue.as_ref().map(|_|json!({"position":self.world(crate::theatre::MICROPHONE_CAPSULE),"forward":[0.,0.,-1.],"up":[0.,1.,0.]}))
    }
    pub fn spawn(&self,eye:f64)->Option<V> {self.venue.as_ref().map(|_|self.world([0.,6.+eye,-52.]))}
    pub fn cabinets(&self)->Vec<crate::audio::line_array::Cabinet> {
        self.venue.as_ref().map(|v|v.speakers.iter().enumerate().map(|(i,s)| {
            let (sy,cy)=s.yaw.to_radians().sin_cos();let(st,ct)=s.tilt.to_radians().sin_cos();
            let forward=[sy*ct,-st,cy*ct];let up=[sy*st,ct,cy*st];
            crate::audio::line_array::Cabinet{id:i as u64+1,bus:s.bus as usize,
                position:self.world(add(s.position,mul(forward,s.size[2]*0.51+0.02))),forward,up,
                horizontal:s.horizontal.map(|n|n as f32),vertical:s.vertical.map(|n|n as f32),band_gain:s.bands.map(|n|n as f32),gain:s.gain as f32}
        }).collect()).unwrap_or_default()
    }
}
pub(crate) type Mesh = Option<(V,Arc<crate::theatre::Venue>,Arc<crate::theatre::office_chair::Chair>)>;
pub(crate) fn mesh(state:&State,input:&Value)->Result<Mesh,String> {
    let eye=vector(&input["position"])?;
    if let Some(city)=crate::theatre::regional_cities::nearby(eye,6_000.) {
        let mut chair=crate::theatre::office_chair::Chair::default();chair.vertices.clear();
        return Ok(Some((city.origin,city.venue.clone(),Arc::new(chair))));
    }
    Ok(if length(state.local(eye))>1500. {None}else{state.venue.as_ref().map(|v|(state.origin,v.clone(),state.chair.clone()))})
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String> {
    match hook {
        "Configure"=>{
            let mut venue=crate::theatre::load(host.root())?;
            // A foundation clears the continuous terrain rather than replacing it.
            let origin=crate::theatre::waterpark::theatre_origin();
            let outside=unit([0.,crate::terrain::RADIUS,-94.]);
            let outside_y=outside[1]*(crate::terrain::RADIUS+crate::terrain::height(outside))-origin[1];
            venue.add_approach(outside_y.min(0.))?;
            if host.mods.enabled("prismals") {
                let content_root=host.mods.content_root("prismals").ok_or("Prismals content root is missing")?;
                let packet=host.mods.call("prismals","Prismals.BuildWorld",&json!({"origin":origin,"outside":outside_y.min(0.),"seed":venue.mod_seed()}))?;
                venue.apply_mod_scene(&packet["scene"])?;
                host.theatre=State{chair:Default::default(),nightworld:Some(Arc::new(crate::theatre::nightworld::Site::load(&content_root)?)),monitor_image:None,origin,flow:Default::default(),venue:Some(Arc::new(venue))};
                let clearings:Vec<(V,f64)>=serde_json::from_value(packet["clearings"].clone()).map_err(|e|e.to_string())?;
                if clearings.len()>65536||clearings.iter().any(|(p,r)|p.iter().any(|v|!v.is_finite())||!r.is_finite()||*r<=0.){return Err("invalid mod clearings".into());}
                for clearing in clearings{if !host.vegetation.clearings.contains(&clearing){host.vegetation.clearings.push(clearing);}}
                host.vegetation.revision=host.vegetation.revision.wrapping_add(1);
            } else {
                host.theatre=State{chair:Default::default(),nightworld:None,monitor_image:None,origin,flow:Default::default(),venue:Some(Arc::new(venue))};
            }
            let center=[0.,crate::terrain::RADIUS,0.];
            if !host.vegetation.clearings.iter().any(|(c,r)|*c==center && *r==82.) {
                host.vegetation.clearings.push((center,82.));
                host.vegetation.revision=host.vegetation.revision.wrapping_add(1);
            }
            Ok(Value::Null)
        },
        "Status"=>Ok(json!({"ready":host.theatre.venue.is_some(),"origin":host.theatre.origin,"computer":host.theatre.computer_position(),"cabinet_count":host.theatre.venue.as_ref().map_or(0,|v|v.speakers.len())})),
        "Mesh"=>{
            let data=mesh(&host.theatre,input)?;
            Ok(data.map_or(Value::Null,|(origin,v,chair)|json!({"origin":origin,"vertices":v.vertices,"chair_vertices":chair.vertices})))
        },
        "ResolveBody"=>{
            let previous=vector(&input["previous"])?;let position=vector(&input["position"])?;
            let eye=input["eye_height"].as_f64().filter(|n|n.is_finite() && (0.2..=4.).contains(n)).ok_or("bounded eye height required")?;
            if let Some(city)=crate::theatre::regional_cities::nearby(position,1_000.) {
                let (p,normals)=city.venue.resolve_body(sub(previous,city.origin),sub(position,city.origin),eye);
                return Ok(json!({"position":add(p,city.origin),"normals":normals}));
            }
            let Some(venue)=&host.theatre.venue else{return Ok(json!({"position":position,"normals":[]}));};
            if length(host.theatre.local(position))>1500. {return Ok(json!({"position":position,"normals":[]}));}
            let local_previous=host.theatre.local(previous);let local_position=host.theatre.local(position);
            let (p,mut normals)=venue.resolve_body(local_previous,local_position,eye);
            let resolved=Arc::make_mut(&mut host.theatre.chair).push(local_previous,p,eye,venue);
            if length(sub(resolved,p))>1e-8 {normals.push(unit(sub(resolved,p)));}
            let p=resolved;
            Ok(json!({"position":host.theatre.world(p),"normals":normals}))
        },
        "ChairInput"=>{
            let eye=host.theatre.local(vector(&input["position"])?);
            let direction=unit(vector(&input["direction"])?);
            let delta=input["wheel"].as_f64().filter(|v|v.is_finite()).ok_or("finite chair wheel delta required")?;
            let targeted=host.theatre.venue.as_ref().is_some_and(|v|host.theatre.chair.targeted(eye,direction,v));
            if targeted && delta!=0. {Arc::make_mut(&mut host.theatre.chair).adjust(delta);}
            Ok(json!({"targeted":targeted,"changed":targeted&&delta!=0.,"height":host.theatre.chair.height}))
        },
        "Trace"=>{
            let position=vector(&input["position"])?;let direction=unit(vector(&input["direction"])?);
            let distance=input["distance"].as_f64().filter(|n|n.is_finite()&&(0.0..=512.).contains(n)).ok_or("bounded theatre trace required")?;
            if let Some(city)=crate::theatre::regional_cities::nearby(position,1_100.) {
                let hit=city.venue.trace_local(sub(position,city.origin),direction,distance);
                return Ok(hit.map_or(Value::Null,|h|json!({"position":add(city.origin,h.position),"normal":h.normal,"distance":h.distance})));
            }
            let hit=host.theatre.venue.as_ref().and_then(|v|v.trace_local(host.theatre.local(position),direction,distance));
            Ok(hit.map_or(Value::Null,|h|json!({"position":host.theatre.world(h.position),"normal":h.normal,"distance":h.distance})))
        },
        "AcousticGeometry"=>{
            let position=vector(&input["position"])?;
            if let Some(city)=crate::theatre::regional_cities::nearby(position,1_000.) {return Ok(city.venue.geometry_snapshot(city.origin));}
            if length(host.theatre.local(position))>700.{return Ok(Value::Null);}
            Ok(host.theatre.venue.as_ref().map_or(Value::Null,|v|v.geometry_snapshot(host.theatre.origin)))
        },
        _=>Err("unknown theatre hook".into())
    }
}
