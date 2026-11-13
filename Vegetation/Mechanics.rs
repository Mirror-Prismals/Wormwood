//! Bounded rigid wood groups, tension-only root fibres and accepted triangle
//! contact. This deliberately reduced model does not replace the OGC soil/tool
//! solver or claim two-way coupling to it. All units are metres, seconds, kg, N.
use super::{math::*, PlantState, Site, Tree, initial_state, profiles::PROFILES};
use serde_json::{json,Value};
use std::{collections::{BTreeMap,BTreeSet},sync::Arc};
use crate::water::{hydrology,types::Snapshot};
const DENSITY: f64 = 550.;
const MAX_BODIES: usize = 32;
const MAX_BRANCHES: usize = 512;
const MAX_FORCE: f64 = 700.;
const ROOT_STIFFNESS: f64 = 16000.;
fn rotate(v:V,w:V)->V {
    let a=length(w); if a<1e-12 {return add(v,cross(w,v));}
    let n=mul(w,1./a); add(add(mul(v,a.cos()),mul(cross(n,v),a.sin())),mul(n,dot(n,v)*(1.-a.cos())))
}
fn vector(v:&Value)->Result<V,String>{crate::water::types::vector(v)}
fn bounded(v:&Value,key:&str,default:f64,low:f64,high:f64)->Result<f64,String>{crate::water::types::number(v,key,default,low,high)}
fn capped(v:V,maximum:f64)->V{mul(v,(maximum/length(v).max(maximum)).min(1.))}
#[derive(Clone)]
struct Root {point:V,anchor:V,rest:f64,broken:bool}
#[derive(Clone)]
pub struct Body {
    id:u64, site:Site, age:f64, branches:Vec<super::branch_graph::Branch>,
    center:V, rest_center:V, axes:[V;3], velocity:V, angular:V,
    mass:f64, volume:f64, inertia:f64, roots:Vec<Root>, pub rooted:bool,
}
impl Body {
    fn make(id:u64,tree:&Tree,age:f64,rooted:bool)->Self {
        let mut mass=0.; let mut weighted=[0.;3];
        for b in &tree.branches {let r=b.radius;let q=r*if b.order==0{0.77}else{0.36};let m=std::f64::consts::PI*length(sub(b.end,b.start))*(r*r+r*q+q*q)/3.*DENSITY;mass+=m;weighted=add(weighted,mul(add(tree.world(b.start),tree.world(b.end)),m*0.5));}
        let center=mul(weighted,1./mass.max(1e-9));
        let mut inertia=0.;
        for b in &tree.branches {let r=b.radius;let q=r*if b.order==0{0.77}else{0.36};let len=length(sub(b.end,b.start));let m=std::f64::consts::PI*len*(r*r+r*q+q*q)/3.*DENSITY;let offset=sub(mul(add(tree.world(b.start),tree.world(b.end)),0.5),center);inertia+=m*(len*len/12.+dot(offset,offset)+r*r*0.5);}
        let mut body=Self{id,site:tree.site,age,branches:tree.branches.clone(),center,rest_center:center,axes:[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]],velocity:[0.;3],angular:[0.;3],mass,volume:mass/DENSITY,inertia:inertia.max(0.01),roots:Vec::new(),rooted};
        if rooted {
            let (e,n)=basis(tree.site.normal);let radius=(tree.height*0.025).clamp(0.12,0.5);
            for i in 0..6 {let a=i as f64*std::f64::consts::TAU/6.;let side=add(mul(e,a.cos()),mul(n,a.sin()));let p=add(tree.site.origin,mul(side,radius));let anchor=add(tree.site.origin,add(mul(side,radius+0.45),mul(tree.site.normal,-0.4)));body.roots.push(Root{point:sub(p,center),anchor,rest:length(sub(p,anchor)),broken:false});}
        }
        body
    }
    fn direction(&self,p:V)->V{add(add(mul(self.axes[0],p[0]),mul(self.axes[1],p[1])),mul(self.axes[2],p[2]))}
    fn point(&self,p:V)->V{add(self.center,self.direction(p))}
    fn inverse_point(&self,p:V)->V{let d=sub(p,self.center);[dot(d,self.axes[0]),dot(d,self.axes[1]),dot(d,self.axes[2])]}
    pub fn tree(&self)->Tree {
        let plant=PlantState{age_years:self.age,alive:true,broken_branches:BTreeSet::new(),vacant_years:0.,generation:0};
        let mut t=Tree::generate(self.site,&plant,super::profiles::profile(self.site.profile));
        t.branches=self.branches.clone();
        let (e,n)=basis(self.site.normal);
        t.site.origin=self.point(sub(self.site.origin,self.rest_center));
        t.site.normal=self.direction(self.site.normal);
        t.frame=Some([self.direction(e),self.direction(self.site.normal),self.direction(n)]);
        t
    }
    fn samples(&self)->Vec<V>{
        let mut result=Vec::new();let t=self.tree();
        for b in &t.branches {let axis=unit(sub(b.end,b.start));let(e,n)=basis(axis);let steps=(length(sub(b.end,b.start))/1.).ceil().clamp(1.,8.) as usize;
            for k in 0..=steps {let f=k as f64/steps as f64;let p=add(b.start,mul(sub(b.end,b.start),f));let r=b.radius*(1.-f+f*if b.order==0{0.77}else{0.36});
                for side in 0..6 {let a=side as f64*std::f64::consts::TAU/6.;result.push(self.inverse_point(t.world(add(p,mul(add(mul(e,a.cos()),mul(n,a.sin())),r)))));}
            }
        } result
    }
    fn snapshot(&self)->Value{json!({"id":self.id,"site_id":self.site.id,"age":self.age,"branches":self.branches.iter().map(|b|b.id).collect::<Vec<_>>(),"center":self.center,"rest_center":self.rest_center,"axes":self.axes,"velocity":self.velocity,"angular":self.angular,"rooted":self.rooted,"mass":self.mass,"volume":self.volume,"roots":self.roots.iter().map(|r|json!({"point":r.point,"anchor":r.anchor,"rest":r.rest,"broken":r.broken})).collect::<Vec<_>>()})}
}
#[derive(Clone)]
struct Grab {body:u64,branch:u32,point:V,distance:f64}
#[derive(Clone,Default)]
pub struct World {
    pub bodies:Vec<Body>, next_id:u64, grab:Option<Grab>,
    contact:Option<Arc<hydrology::State>>,contact_key:Option<(u64,V)>,
    pub ecological_seconds:f64,
}
impl World {
    pub fn rooted_tree(&self,id:u64)->Option<Tree>{self.bodies.iter().find(|b|b.site.id==id&&b.rooted).map(Body::tree)}
    pub fn fallen(&self)->impl Iterator<Item=Tree>+'_ {self.bodies.iter().filter(|b|!b.rooted).map(Body::tree)}
    pub fn snapshot(&self)->Value{json!({"schema_version":1,"next_id":self.next_id,"ecological_seconds":self.ecological_seconds,"bodies":self.bodies.iter().map(Body::snapshot).collect::<Vec<_>>()})}
    pub fn restore(v:&Value)->Result<Self,String>{
        if v.is_null(){return Ok(Self::default());}
        if v["schema_version"]!=1{return Err("unsupported wood state version".into());}
        let rows=v["bodies"].as_array().filter(|a|a.len()<=MAX_BODIES).ok_or("bounded wood bodies required")?;
        let mut out=Self::default();let mut used=BTreeSet::new();let mut wood=BTreeSet::new();
        for row in rows {
            let site=super::site_by_id(row["site_id"].as_u64().ok_or("wood site required")?).ok_or("unknown wood site")?;
            let age=bounded(row,"age",0.,0.,10000.)?;let plant=PlantState{age_years:age,alive:true,broken_branches:BTreeSet::new(),vacant_years:0.,generation:0};let mut tree=Tree::generate(site,&plant,super::profiles::profile(site.profile));
            let ids=row["branches"].as_array().filter(|a|!a.is_empty()&&a.len()<=super::branch_count(site.profile)).ok_or("wood branches required")?.iter().map(|id|id.as_u64().filter(|id|*id<super::branch_count(site.profile) as u64).map(|id|id as u32).ok_or("invalid wood branch")).collect::<Result<BTreeSet<_>,_>>()?;
            for id in &ids{if !wood.insert((site.id,*id)){return Err("duplicate retained wood material".into());}}
            tree.branches.retain(|b|ids.contains(&b.id));if tree.branches.len()!=ids.len(){return Err("wood branch topology mismatch".into());}
            let id=row["id"].as_u64().filter(|id|*id>0&&*id<u64::MAX).ok_or("wood body id required")?;if !used.insert(id){return Err("duplicate wood body".into());}
            let mut body=Body::make(id,&tree,age,false);body.center=vector(&row["center"])?;body.rest_center=vector(&row["rest_center"])?;body.velocity=vector(&row["velocity"])?;body.angular=vector(&row["angular"])?;
            body.axes=serde_json::from_value(row["axes"].clone()).map_err(|_|"wood axes required")?;
            if body.axes.iter().flatten().any(|n|!n.is_finite())||body.axes.iter().any(|a|(length(*a)-1.).abs()>1e-5)||dot(body.axes[0],body.axes[1]).abs()>1e-5||dot(cross(body.axes[0],body.axes[1]),body.axes[2])<0.99999{return Err("invalid wood rotation".into());}
            if (bounded(row,"mass",body.mass,0.,1e9)?-body.mass).abs()>body.mass*1e-6|| (bounded(row,"volume",body.volume,0.,1e9)?-body.volume).abs()>body.volume*1e-6{return Err("wood mass/volume mismatch".into());}
            body.rooted=row["rooted"].as_bool().ok_or("wood rooted flag required")?;
            let roots=row["roots"].as_array().filter(|a|a.len()<=6).ok_or("bounded root fibres required")?;
            for r in roots{body.roots.push(Root{point:vector(&r["point"])?,anchor:vector(&r["anchor"])?,rest:bounded(r,"rest",0.,0.01,4.)?,broken:r["broken"].as_bool().ok_or("root state required")?});}
            if body.rooted&&(body.roots.len()!=6||body.roots.iter().all(|r|r.broken)){return Err("rooted wood has no root support".into());}
            out.next_id=out.next_id.max(id);out.bodies.push(body);
        }
        if out.bodies.iter().map(|b|b.branches.len()).sum::<usize>()>MAX_BRANCHES{return Err("wood branch budget exceeded".into());}
        out.next_id=out.next_id.max(v["next_id"].as_u64().unwrap_or(0));out.ecological_seconds=bounded(v,"ecological_seconds",0.,0.,1e12)?;Ok(out)
    }
    fn acquire(&mut self,plants:&BTreeMap<u64,PlantState>,eye:V,aim:V){
        let mut best=4.;let mut selected=None;
        let mut trees:Vec<_>=super::sites_near(eye,40.,128).into_iter().filter_map(|s|{
            let p=plants.get(&s.id).cloned().unwrap_or_else(||initial_state(s));p.alive.then(||Tree::generate(s,&p,super::profiles::profile(s.profile)))
        }).collect();
        // Existing active bodies must be picked in their actual accepted pose.
        trees.retain(|t|!self.bodies.iter().any(|b|b.site.id==t.site.id&&b.rooted));
        trees.extend(self.bodies.iter().map(Body::tree));
        for t in trees {for b in &t.branches {let a=t.world(b.start);let end=t.world(b.end);let segment=sub(end,a);let l=dot(segment,segment);if l<1e-12{continue;}
            // Minimize ray/segment separation with clamped projections.
            let mut distance=dot(sub(a,eye),aim).clamp(0.,4.);
            for _ in 0..4 {let point=add(eye,mul(aim,distance));let f=(dot(sub(point,a),segment)/l).clamp(0.,1.);let q=add(a,mul(segment,f));distance=dot(sub(q,eye),aim).clamp(0.,4.);}
            let ray=add(eye,mul(aim,distance));let f=(dot(sub(ray,a),segment)/l).clamp(0.,1.);let q=add(a,mul(segment,f));
            if distance<best&&length(sub(ray,q))<=b.radius+0.08 {best=distance;selected=Some((t.clone(),b.id,q));}
        }}
        if let Some((tree,branch,point))=selected {
            let body=if let Some(b)=self.bodies.iter().find(|b|b.site.id==tree.site.id&&b.branches.iter().any(|v|v.id==branch)){b.id}else{
                if self.bodies.len()>=MAX_BODIES||self.bodies.iter().map(|b|b.branches.len()).sum::<usize>()+tree.branches.len()>MAX_BRANCHES{return;}
                self.next_id+=1;let age=plants.get(&tree.site.id).cloned().unwrap_or_else(||initial_state(tree.site)).age_years;
                self.bodies.push(Body::make(self.next_id,&tree,age,true));self.next_id
            };
            let b=self.bodies.iter().find(|b|b.id==body).unwrap();self.grab=Some(Grab{body,branch,point:b.inverse_point(point),distance:best});
        }
    }
    pub fn advance(&mut self,plants:&mut BTreeMap<u64,PlantState>,input:&Value,snapshot:Snapshot)->Result<Value,String>{
        let eye=vector(&input["position"])?;let aim=unit(vector(&input["forward"])?);if length(aim)<0.9{return Err("tree aim must be nonzero".into());}
        let dt=bounded(input,"dt",0.,0.,0.05)?;let grab=input["grab"]==true;
        let key=(snapshot.revision,snapshot.origin);
        if self.contact_key.as_ref()!=Some(&key){let mut contact=hydrology::State::default();contact.update(snapshot,&crate::water::slides::Mesh::default(),0);self.contact=Some(Arc::new(contact));self.contact_key=Some(key);}
        let contact=self.contact.as_ref().unwrap().clone();
        if !grab{self.grab=None;}else if self.grab.is_none(){self.acquire(plants,eye,aim);}
        let mut changed=false;let mut broken_roots=0usize;let mut fracture=None;
        let steps=(dt*120.).ceil().clamp(1.,6.) as usize;let h=dt/steps as f64;
        let target=self.grab.as_ref().map(|g|add(eye,mul(aim,g.distance)));
        let mut feedback=[0.;3];let mut dormant=0;
        for body in &mut self.bodies {
            if length(sub(body.center,eye))>64. {dormant+=1;continue;}
            let samples=body.samples();
            if !samples.iter().all(|p|contact.covered(body.point(*p),0.005)){dormant+=1;continue;}
            // Actual accepted soil removal severs fibres whose embedded anchor
            // is exposed. No digging gesture or appearance flag does this.
            for i in 0..body.roots.len(){if body.roots[i].broken{continue;}let r=&body.roots[i];let up=unit(r.anchor);let origin=add(r.anchor,mul(up,2.));
                if contact.ray(origin,mul(up,-1.),4.).is_none_or(|hit|dot(sub(hit.point,r.anchor),up)<0.1){body.roots[i].broken=true;broken_roots+=1;}
            }
            for _ in 0..steps {
                if h<=0. {break;}
                let old=body.clone();let mut force=mul(unit(body.center),-9.81*body.mass);let mut torque=[0.;3];
                for i in 0..body.roots.len(){let r=&body.roots[i];if r.broken{continue;}let point=body.point(r.point);let delta=sub(r.anchor,point);let distance=length(delta);let stretch=(distance-r.rest).max(0.);let tension=ROOT_STIFFNESS*stretch;
                    let capacity=180.*(0.3+0.7*body.site.soil_depth.clamp(0.,1.));
                    if tension>capacity {body.roots[i].broken=true;broken_roots+=1;continue;}
                    let f=mul(unit(delta),tension);force=add(force,f);torque=add(torque,cross(sub(point,body.center),f));
                }
                if let (Some(g),Some(target))=(&self.grab,target){if g.body==body.id {
                    let p=body.point(g.point);let velocity=add(body.velocity,cross(body.angular,sub(p,body.center)));
                    let f=capped(sub(mul(sub(target,p),4500.),mul(velocity,120.)),MAX_FORCE);feedback=f;force=add(force,f);torque=add(torque,cross(sub(p,body.center),f));
                    if let Some(branch)=body.branches.iter().find(|b|b.id==g.branch){let root=body.tree().world(branch.start);let moment=length(cross(sub(p,root),f));let capacity=5_000_000.*std::f64::consts::PI*branch.radius.powi(3)/4.;if moment>capacity&&branch.order>0 {fracture=Some((body.id,branch.id));}}
                }}
                body.velocity=capped(add(body.velocity,mul(force,h/body.mass.max(0.01))),20.);
                body.angular=capped(mul(add(body.angular,mul(torque,h/body.inertia)),(-0.6*h).exp()),6.);
                body.center=add(body.center,mul(body.velocity,h));
                let rotation=mul(body.angular,h);for axis in &mut body.axes{*axis=rotate(*axis,rotation);}
                body.axes[0]=unit(body.axes[0]);body.axes[1]=unit(sub(body.axes[1],mul(body.axes[0],dot(body.axes[0],body.axes[1]))));body.axes[2]=unit(cross(body.axes[0],body.axes[1]));
                let mut valid=true;
                for local in &samples {let p=body.point(*local);if !contact.covered(p,0.005){valid=false;break;}
                    if let Some(hit)=contact.sweep(old.point(*local),p,0.003){let correction=sub(hit.point,p);if length(correction)>0.5{valid=false;break;}body.center=add(body.center,correction);
                        let arm=sub(hit.point,body.center);let speed=dot(add(body.velocity,cross(body.angular,arm)),hit.normal);
                        if speed<0. {let lever=cross(arm,hit.normal);let j=-speed/(1./body.mass+dot(lever,lever)/body.inertia);let impulse=mul(hit.normal,j);body.velocity=add(body.velocity,mul(impulse,1./body.mass));body.angular=add(body.angular,mul(cross(arm,impulse),1./body.inertia));body.velocity=mul(body.velocity,(-3.*h).exp());}
                    }
                }
                if !valid{*body=old;body.velocity=[0.;3];body.angular=[0.;3];break;}
                changed|=length(sub(body.center,old.center))>1e-9||length(sub(body.axes[0],old.axes[0]))>1e-9;
            }
            if body.rooted&&body.roots.iter().all(|r|r.broken){body.rooted=false;let p=plants.entry(body.site.id).or_insert_with(||initial_state(body.site));p.alive=false;p.vacant_years=0.;changed=true;}
        }
        if let Some((body_id,branch))=fracture {
            if self.bodies.len()<MAX_BODIES {if let Some(index)=self.bodies.iter().position(|b|b.id==body_id){
                let parent=&self.bodies[index];let mut ids=BTreeSet::from([branch]);for b in &parent.branches{if b.parent.is_some_and(|id|ids.contains(&id)){ids.insert(b.id);}}
                let mut tree=parent.tree();tree.branches.retain(|b|ids.contains(&b.id));
                // Rebase to the original site's local coordinates while keeping
                // each accepted world endpoint and mass in the new body.
                let mut child=parent.clone();self.next_id+=1;child.id=self.next_id;child.branches.retain(|b|ids.contains(&b.id));child.roots.clear();child.rooted=false;
                // Recompute exact group mass/inertia without changing the pose.
                let mut rest=Tree::generate(parent.site,&PlantState{age_years:parent.age,alive:true,broken_branches:BTreeSet::new(),vacant_years:0.,generation:0},super::profiles::profile(parent.site.profile));rest.branches=child.branches.clone();let mass_body=Body::make(child.id,&rest,parent.age,false);
                child.mass=mass_body.mass;child.volume=mass_body.volume;child.inertia=mass_body.inertia;
                let mut remaining=parent.clone();remaining.branches.retain(|b|!ids.contains(&b.id));rest.branches=remaining.branches.clone();let retained_mass=Body::make(remaining.id,&rest,parent.age,remaining.rooted);
                remaining.mass=retained_mass.mass;remaining.volume=retained_mass.volume;remaining.inertia=retained_mass.inertia;
                self.bodies[index]=remaining;self.bodies.push(child);
                let site=self.bodies[index].site;plants.entry(site.id).or_insert_with(||initial_state(site)).broken_branches.insert(branch);
                self.grab=None;changed=true;
            }}
        }
        Ok(json!({"active":self.grab.is_some(),"changed":changed||broken_roots>0,"force":feedback,"broken_roots":broken_roots,"bodies":self.bodies.len(),"dormant_bodies":dormant,"mass_kg":self.bodies.iter().map(|b|b.mass).sum::<f64>(),"volume_m3":self.bodies.iter().map(|b|b.volume).sum::<f64>(),"contact_model":"reduced_rigid_wood_continuous_accepted_triangle_contact"}))
    }
}
