//! Bounded rooted sod material for OGC contact experiments. Elasticity, grasp
//! and failure are CPU XPBD; OGC supplies contact. Parameters are artistic,
//! not measured turf properties. No material is deleted on failure or release.
use super::{Scene, math::*};
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tissue { Root, Soil }
#[derive(Clone)]
struct Bond {
    a:usize, b:usize, rest:V, lambda:V, compliance:f32,
    strength:f32, extension_limit:f32, peak:f32, broken:bool, tissue:Tissue,
}
#[derive(Clone)]
struct Grasp { nodes:Vec<(usize,V,V)>, target:V }
#[derive(Clone)]
pub(crate) struct Material {
    bonds:Vec<Bond>, blade_tips:Vec<usize>, hand:[usize;2],
    soil:std::ops::Range<usize>, soil_rest:Vec<V>, mass:f32,
    grasp:Option<Grasp>, palm:V, closing:bool, contact_ready:bool,
    contacts:u64, peak_grasp_force:f32, released:bool,
    /// The workshop supplies a trajectory; the material itself accepts arbitrary
    /// targets through control(), with no camera/gesture driven deletion.
    pub(crate) workshop_driver:bool,
}
impl Material {
    fn bond(&mut self, scene:&Scene, a:usize,b:usize,tissue:Tissue,strength:f32) {
        self.bonds.push(Bond {a,b,rest:sub(scene.p[b],scene.p[a]),lambda:[0.;3],
            compliance:if tissue==Tissue::Root {2e-7}else{8e-7},
            strength,extension_limit:if tissue==Tissue::Root {0.035}else{0.018},
            peak:0.,broken:false,tissue});
    }
    pub(crate) fn control(&mut self, target:V, closed:bool) {
        self.palm=target; self.closing=closed;
        if !closed {self.grasp=None;self.released=true;}
        if let Some(g)=&mut self.grasp {g.target=target;}
    }
    pub(crate) fn drive(&mut self, scene:&mut Scene) {
        if self.workshop_driver {
            let t=scene.time;
            self.control([0.,0.285+((t-0.8).max(0.)*0.055).min(0.18),0.],t>=0.45 && t<4.5);
        }
        // Two finite finger pads are contact surfaces. Closure stops short of
        // the grass midplane; contacts are never replaced by a collision flag.
        let gap=if self.closing {0.0015} else {0.045};
        for (pad,&start) in self.hand.iter().enumerate() {
            let sign=if pad==0 {-1.}else{1.};
            let wanted=add(self.palm,[0.,0.,sign*(gap+0.012)]);
            let current=scene.p[start..start+8].iter().copied().fold([0.;3],add).map(|v|v/8.);
            let delta=sub(wanted,current);
            let movement=scale(delta,(0.0008/length(delta).max(0.0008)).min(1.));
            for p in &mut scene.p[start..start+8] {*p=add(*p,movement);}
        }
    }
    pub(crate) fn begin_step(&mut self) {
        for b in &mut self.bonds {b.lambda=[0.;3];}
        if let Some(g)=&mut self.grasp {for n in &mut g.nodes {n.2=[0.;3];}}
    }
    pub(crate) fn observe_contact(&mut self, scene:&Scene, contacts:u64) {
        self.contacts=self.contacts.max(contacts);
        if self.closing && self.grasp.is_none() && contacts>0 {
            let mut nodes=Vec::new();
            for &i in &self.blade_tips {
                let p=scene.p[i];
                // Grasp only tissue between the actual accepted finger pads,
                // not all grass under a screen-space cursor.
                let a=scene.p[self.hand[0]..self.hand[0]+8].iter().copied().fold([0.;3],add).map(|x|x/8.);
                let b=scene.p[self.hand[1]..self.hand[1]+8].iter().copied().fold([0.;3],add).map(|x|x/8.);
                let center=scale(add(a,b),0.5);
                if (p[0]-center[0]).abs()<0.075 && (p[1]-center[1]).abs()<0.035
                    && p[2]>=a[2] && p[2]<=b[2] && (b[2]-a[2])<0.04 {
                    nodes.push((i,sub(p,self.palm),[0.;3]));
                }
            }
            if !nodes.is_empty() {self.grasp=Some(Grasp{nodes,target:self.palm});self.contact_ready=true;}
        }
    }
    pub(crate) fn solve(&mut self,scene:&mut Scene,dt:f32) {
        let h2=dt*dt;
        for b in &mut self.bonds {
            if b.broken {continue;}
            let delta=sub(scene.p[b.b],scene.p[b.a]);
            let difference=if b.tissue==Tissue::Root {
                let l=length(delta);let r=length(b.rest);if l<=r {continue;} scale(delta,(l-r)/l.max(1e-8))
            } else {sub(delta,b.rest)};
            let alpha=b.compliance/h2;
            let denom=scene.weights[b.a]+scene.weights[b.b]+alpha;
            if denom<=0. {continue;}
            let dl=scale(add(scale(difference,-1.),scale(b.lambda,-alpha)),1./denom);
            let next=add(b.lambda,dl);let force=length(next)/h2;
            b.peak=b.peak.max(force);
            if length(difference)>b.extension_limit || (force>b.strength && length(difference)>0.0002) {
                b.broken=true;continue;
            }
            b.lambda=next;
            scene.p[b.a]=sub(scene.p[b.a],scale(dl,scene.weights[b.a]));
            scene.p[b.b]=add(scene.p[b.b],scale(dl,scene.weights[b.b]));
        }
        if let Some(g)=&mut self.grasp {
            let max_lambda=60.*h2/g.nodes.len().max(1) as f32;
            let mut load=0.;
            for (i,offset,lambda) in &mut g.nodes {
                let difference=sub(scene.p[*i],add(g.target,*offset));
                let alpha=5e-8/h2;
                let dl=scale(add(scale(difference,-1.),scale(*lambda,-alpha)),1./(scene.weights[*i]+alpha));
                let next=add(*lambda,dl);
                let next=scale(next,(max_lambda/length(next).max(max_lambda)).min(1.));
                scene.p[*i]=add(scene.p[*i],scale(sub(next,*lambda),scene.weights[*i]));
                *lambda=next;load+=length(next)/h2;
            }
            self.peak_grasp_force=self.peak_grasp_force.max(load);
        }
    }
    pub(crate) fn report(&self,scene:&Scene)->Value {
        let soil_lift=self.soil.clone().zip(&self.soil_rest).map(|(i,p)|scene.p[i][1]-p[1]).fold(f32::NEG_INFINITY,f32::max);
        let roots=self.bonds.iter().filter(|b|b.tissue==Tissue::Root);
        let soil=self.bonds.iter().filter(|b|b.tissue==Tissue::Soil);
        json!({"grasped":self.grasp.is_some(),"grasp_acquired_after_contact":self.contact_ready,"released":self.released,
            "roots":roots.clone().count(),"roots_broken":roots.filter(|b|b.broken).count(),
            "soil_attachments":soil.clone().count(),"soil_attachments_broken":soil.clone().filter(|b|b.broken).count(),
            "sod_detached":soil.clone().all(|b|b.broken),"soil_lift_metres":soil_lift,
            "peak_grasp_force_newtons":self.peak_grasp_force,"peak_root_load_newtons":self.bonds.iter().filter(|b|b.tissue==Tissue::Root).map(|b|b.peak).fold(0f32,f32::max),
            "material_mass_kg":self.mass,"retained_nodes":scene.p.len(),"native_contact_pairs_peak":self.contacts,
            "failure_model":"tension_limited_roots_and_cohesive_sod_attachment","calibrated":false})
    }
}

fn block(scene:&mut Scene,min:V,max:V,mass:f32,color:V)->usize {
    let start=scene.p.len();
    for y in [min[1],max[1]] {for z in [min[2],max[2]] {for x in [min[0],max[0]] {scene.node([x,y,z],mass/8.,color);}}}
    scene.surface(start,vec![[0,2,1],[1,2,3],[4,5,6],[5,7,6],[0,1,4],[1,5,4],[2,6,3],[3,6,7],[0,4,2],[2,4,6],[1,3,5],[3,7,5]],mass==0.,false);
    start
}
/// Build a local material sample. There is no prescribed blade-tip motion.
pub(crate) fn fixture(scene:&mut Scene) {
    scene.radius=0.002;scene.substeps=4;scene.iterations=8;
    let bottom=block(scene,[-0.10,0.04,-0.08],[0.10,0.115,0.08],0.,[0.24,0.16,0.08]);
    let sod=block(scene,[-0.08,0.121,-0.06],[0.08,0.161,0.06],0.80,[0.32,0.21,0.11]);
    for tet in [[0,1,3,7],[0,3,2,7],[0,2,6,7],[0,6,4,7],[0,4,5,7],[0,5,1,7]] {scene.tetra(tet.map(|i|sod+i));}
    // Origin for palm height is near the grass tips, above the intact sod.
    let palm=[0.,0.285,0.];
    let mut material=Material{bonds:vec![],blade_tips:vec![],hand:[0;2],soil:sod..sod+8,soil_rest:scene.p[sod..sod+8].to_vec(),mass:0.80,
        grasp:None,palm,closing:false,contact_ready:false,contacts:0,peak_grasp_force:0.,released:false,workshop_driver:true};
    for i in 0..4 {material.bond(scene,bottom+4+i,sod+i,Tissue::Soil,2.5);}
    for blade in 0..4 {
        let x=-0.045+blade as f32*0.03;let start=scene.p.len();
        for y in [0.166,0.224,0.282] {for side in [-0.007,0.007] {scene.node([x+side,y,0.],0.00015,[0.14,0.40,0.06]);}}
        for layer in 0..3 {scene.edge(start+layer*2,start+layer*2+1,1e-10);}
        for layer in 0..2 {for (a,b) in [(0,0),(1,1),(0,1),(1,0)] {scene.edge(start+layer*2+a,start+(layer+1)*2+b,1e-10);}}
        scene.edge(start,start+4,3e-7);scene.edge(start+1,start+5,3e-7);
        scene.surface(start,vec![[0,1,2],[1,3,2],[2,3,4],[3,5,4]],false,false);
        let soil_top=sod+4+usize::from(blade>=2);
        for i in 0..2 {material.bond(scene,soil_top,start+i,Tissue::Root,12.);}
        material.blade_tips.extend([start+4,start+5]);
        material.mass+=6.*0.00015;
    }
    for side in 0..2 {
        let z=if side==0 {-0.057}else{0.057};
        material.hand[side]=block(scene,[-0.065,palm[1]-0.025,z-0.012],[0.065,palm[1]+0.025,z+0.012],0.,[0.66,0.44,0.29]);
    }
    scene.sod=Some(material);
    scene.proxy="Rooted grass and deformable sod with finite OGC finger-pad contact, force-limited grasp, breakable roots/cohesion and retained material. Workshop controls prescribe the palm path; no playable hand integration or calibrated turf properties.".into();
}
