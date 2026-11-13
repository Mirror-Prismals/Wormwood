//! Wearable workstation access. No Cardinal dimensions, portals or world formats.
use crate::{audio::*,host::Host,phone::Pose};
use serde_json::{Value,json};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Carry {Placed,Held,Worn}
pub(crate) struct Pair {pub kind: &'static str,pub pose:Pose,pub location:Carry}
pub(crate) struct State {pub pairs:Vec<Pair>,previous:[bool;3],owned_f:bool}
impl Default for State {fn default()->Self {Self{pairs:Vec::new(),previous:[false;3],owned_f:false}}}
impl State {
    pub fn carried(&self)->Option<usize>{self.pairs.iter().position(|p|p.location!=Carry::Placed)}
    pub fn owns_interact(&self)->bool{self.owned_f}
    pub fn held(&self)->bool{self.pairs.iter().any(|p|p.location==Carry::Held)}
    pub fn worn_kind(&self)->Option<&'static str>{self.pairs.iter().find(|p|p.location==Carry::Worn).map(|p|p.kind)}
    pub fn worn(&self)->bool{self.pairs.iter().any(|p|p.location==Carry::Worn)}
    fn status(&self)->Value{json!({"held":self.held(),"worn":self.worn(),"pairs":self.pairs.iter().map(|p|json!({"kind":p.kind,"location":format!("{:?}",p.location)})).collect::<Vec<_>>()})}
}
pub(crate) fn presented(pair:&Pair,view:&Value)->Result<Pose,String>{
    if pair.location==Carry::Placed{return Ok(pair.pose);}
    let eye=vector(&view["position"])?;let f=unit(vector(&view["forward"])?);let up=unit(vector(&view["up"])?);let right=unit(cross(f,up));
    let position=if pair.location==Carry::Held{add(eye,add(mul(f,0.52),add(mul(up,-0.19),mul(right,0.25))))}else{add(eye,mul(f,0.055))};
    Pose::new(position,if pair.location==Carry::Worn{mul(right,-1.)}else{right},up)
}
// Two-sided exact mesh picking, including the open space inside AR rims.
pub(crate) fn hit(vertices:&[[f32;6]],pose:Pose,eye:V,ray:V)->Option<f64>{
    let point=|v:[f32;6]|add(pose.position,add(mul(pose.right,v[0]as f64),add(mul(pose.up,v[1]as f64),mul(pose.front,v[2]as f64))));
    vertices.chunks_exact(3).filter_map(|t|{let a=point(t[0]);let e1=sub(point(t[1]),a);let e2=sub(point(t[2]),a);let h=cross(ray,e2);let det=dot(e1,h);if det.abs()<1e-10{return None;}
        let s=sub(eye,a);let u=dot(s,h)/det;if !(0.0..=1.).contains(&u){return None;}let q=cross(s,e1);let v=dot(ray,q)/det;if v<0.||u+v>1.{return None;}let d=dot(e2,q)/det;(0.02..=3.25).contains(&d).then_some(d)
    }).min_by(f64::total_cmp)
}
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{
    match hook {
        "Configure"=>{host.invoke("GlassesRenderSystem.Load",Value::Null)?;host.glasses=State::default();
            for (kind,x,y) in [("vr",-0.95,7.50),("ar",-1.25,7.477)]{host.glasses.pairs.push(Pair{kind,pose:Pose::new(host.theatre.world([x,y,-47.20]),[-1.,0.,0.],[0.,1.,0.])?,location:Carry::Placed});}Ok(Value::Null)},
        "Status"=>Ok(host.glasses.status()),
        "Close"=>{host.glasses=State::default();Ok(Value::Null)},
        "Grip"=>{
            let Some(p)=host.glasses.pairs.iter().find(|p|p.location==Carry::Held) else{return Ok(json!({"active":false}));};
            let pose=presented(p,input)?;let grip=add(pose.position,mul(pose.up,-0.026));
            Ok(json!({"active":true,"left":add(grip,mul(pose.right,-0.065)),"right":add(grip,mul(pose.right,0.065)),"locked":true}))
        },
        "Input"=>{
            let window=&input["window"];let view=&input["view"];let k=&window["flight"]["glasses"];
            let keys=[k["interact"]==true,k["equip"]==true,k["open"]==true];
            let edge:[bool;3]=std::array::from_fn(|i|keys[i]&&!host.glasses.previous[i]);host.glasses.previous=keys;if !keys[0]{host.glasses.owned_f=false;}
            let blocked=input["blocked"]==true||window["flight"]["focused"]==false||input["phone_focused"]==true;
            let mut changed=false;let mut open=false;
            if !blocked&&view["position"].is_array(){
                let eye=vector(&view["position"])?;let ray=unit(vector(&view["forward"])?);
                if edge[0]&&input["phone_consumed"]!=true {
                    if let Some(i)=host.glasses.carried(){
                        // Place only on an actual supporting surface in reach.
                        let snap=host.invoke("TerrainGeometrySystem.Capture",json!({"position":eye,"time":0.}))?;
                        let target=host.invoke("TerrainGeometrySystem.Trace",json!({"snapshot":snap,"position":eye,"direction":ray,"distance":3.25}))?;
                        if !target.is_null(){let normal=unit(vector(&target["normal"])?);
                            if dot(normal,vector(&view["up"])?)>0.65 {
                                let clearance=if host.glasses.pairs[i].kind=="vr"{0.050}else{0.027};
                                let pos=add(vector(&target["position"] )?,mul(normal,clearance));let mut tangent=cross(ray,normal);
                                if length(tangent)<0.01 {let r=vector(&view["right"])?;tangent=sub(r,mul(normal,dot(r,normal)));}
                                let right=unit(tangent);
                                host.glasses.pairs[i].pose=Pose::new(pos,right,normal)?;host.glasses.pairs[i].location=Carry::Placed;changed=true;
                            }
                        }
                        host.glasses.owned_f=true;
                    }else if input["phone_held"]!=true {
                        let mut hits=Vec::new();
                        for (i,p) in host.glasses.pairs.iter().enumerate(){if let Some(v)=host.glasses_render.models.get(p.kind){if let Some(d)=hit(v,p.pose,eye,ray){hits.push((i,d));}}}
                        hits.sort_by(|a,b|a.1.total_cmp(&b.1));
                        if let Some((i,d))=hits.first().copied(){
                            host.glasses.owned_f=true;
                            let snap=host.invoke("TerrainGeometrySystem.Capture",json!({"position":eye,"time":0.}))?;
                            let obstacle=host.invoke("TerrainGeometrySystem.Trace",json!({"snapshot":snap,"position":eye,"direction":ray,"distance":d}))?;
                            let clear=obstacle["distance"].as_f64().is_none_or(|v|v+0.015>=d);
                            if clear&&host.invoke("PhysicsSystem.GameFrame",Value::Null)?["can_stow_tool"]==true{host.glasses.pairs[i].location=Carry::Held;changed=true;}
                        }
                    }
                }else if edge[1]&&input["phone_held"]!=true {
                    if let Some(i)=host.glasses.carried(){
                        let removing=host.glasses.pairs[i].location==Carry::Worn;
                        if !removing || host.invoke("PhysicsSystem.GameFrame",Value::Null)?["can_stow_tool"]==true {
                            host.glasses.pairs[i].location=if removing{Carry::Held}else{Carry::Worn};changed=true;
                        }
                    }
                }
                open=edge[2]&&host.glasses.worn()&&!changed;
            }
            for pair in &mut host.glasses.pairs {if pair.location!=Carry::Placed && view["position"].is_array(){pair.pose=presented(pair,view)?;}}
            let mut status=host.glasses.status();status["open"]=json!(open);status["changed"]=json!(changed);status["consumed_interact"]=json!(host.glasses.owned_f);status["consumed_open"]=json!(keys[2]&&host.glasses.worn());Ok(status)
        },
        _=>Err(format!("unknown glasses hook {hook}"))
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn mesh_pick_rejects_gap_range_and_behind(){let p=Pose::new([0.,0.,2.],[1.,0.,0.],[0.,1.,0.]).unwrap();let m=[[-0.1,-0.1,0.,1.,1.,1.],[0.1,-0.1,0.,1.,1.,1.],[0.,0.1,0.,1.,1.,1.]];assert_eq!(hit(&m,p,[0.;3],[0.,0.,1.]),Some(2.));assert!(hit(&m,p,[1.,0.,0.],[0.,0.,1.]).is_none());assert!(hit(&m,p,[0.,0.,-2.],[0.,0.,1.]).is_none());assert!(hit(&m,p,[0.;3],[0.,0.,-1.]).is_none());}
}

