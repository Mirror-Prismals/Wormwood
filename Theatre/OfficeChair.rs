//! Independent rolling furniture on the bounded, level front-of-house deck.
use super::*;
#[derive(Clone)]
pub(crate) struct Chair { pub position:V, pub height:f64, pub vertices:Vec<[f32;7]>, wheel_angle:f64 }
impl Default for Chair {
 fn default()->Self {let mut c=Self{position:[0.,6.,-48.65],height:1.18,vertices:Vec::new(),wheel_angle:0.};c.rebuild();c}
}
impl Chair {
 pub fn adjust(&mut self,delta:f64){if delta.is_finite(){self.height=(self.height+delta.clamp(-10.,10.)*0.04).clamp(0.65,1.45);self.rebuild();}}
 pub fn targeted(&self,eye:V,direction:V,venue:&Venue)->bool {
  let b=Solid{center:add(self.position,[0.,self.height+0.25,0.]),size:[0.95,1.25,0.95],yaw:0.};
  b.interval(eye,direction,3.).is_some_and(|(d,_)|venue.trace_local(eye,direction,d).is_none())
 }
 /// Walking pushes the casters. Swept steps and the deck boundary prevent tunnelling/falling.
 pub fn push(&mut self,previous:V,desired:V,eye:f64,venue:&Venue)->V {
  if previous.iter().chain(&desired).any(|v|!v.is_finite()) {return previous;}
  let delta=sub(desired,previous);let steps=(length(delta)/0.08).ceil().clamp(1.,512.)as usize;
  let mut p=previous;let old=self.position;
  for _ in 0..steps {
   p=add(p,mul(delta,1./steps as f64));
   if p[1]-eye>self.position[1]+self.height+0.85 || p[1]<self.position[1]{continue;}
   let d=[p[0]-self.position[0],0.,p[2]-self.position[2]];let distance=length(d);
   if distance>=0.82 {continue;}
   let n=if distance>1e-8 {mul(d,1./distance)}else{[0.,0.,-1.]};
   let shift=mul(n,-(0.82-distance));let candidate=add(self.position,shift);
   // Keep all five casters on the FOH platform. Furniture cannot roll down stairs.
   if candidate[0].abs()<=6.25 && (-54.1..=-44.8).contains(&candidate[2]) {
    // Resolve an expanded chair footprint against the same desk/rail/wall solids.
    let probe=add(self.position,[0.,self.height+0.7,0.]);
    let wanted=add(candidate,[0.,self.height+0.7,0.]);
    let clear=[[-0.23,0.,-0.23],[-0.23,0.,0.23],[0.23,0.,-0.23],[0.23,0.,0.23]].iter().all(|offset| {
     let wanted=add(wanted,*offset);let (resolved,_)=venue.resolve_body(add(probe,*offset),wanted,self.height+0.7);
     length(sub(resolved,wanted))<0.025
    });
    if clear {self.position=candidate;}
   }
   let d=sub(p,self.position);let horizontal=[d[0],0.,d[2]];let distance=length(horizontal);
   if distance<0.82 {let normal=if distance>1e-8{mul(horizontal,1./distance)}else{n};p[0]=self.position[0]+normal[0]*0.82;p[2]=self.position[2]+normal[2]*0.82;}
  }
  let travel=length(sub(self.position,old));if travel>0. {self.wheel_angle+=travel/0.09;self.rebuild();}p
 }
 fn box_part(&mut self,center:V,size:V,color:[f32;3],yaw:f64){
  let b=Solid{center:add(self.position,center),size,yaw};let p:[V;8]=std::array::from_fn(|i|b.world(std::array::from_fn(|a|if i&(1<<a)==0{-size[a]*0.5}else{size[a]*0.5})));
  for f in [[0,4,6,2],[1,3,7,5],[0,1,5,4],[2,6,7,3],[0,2,3,1],[4,5,7,6]] {for t in [[0,1,2],[0,2,3]] {for i in t{let v=p[f[i]];self.vertices.push([v[0]as f32,v[1]as f32,v[2]as f32,color[0],color[1],color[2],0.]);}}}
 }
 fn wheel(&mut self,center:V,yaw:f64){
  let (s,c)=yaw.to_radians().sin_cos();let axle=[c,0.,-s];let radial=[s,0.,c];let center=add(self.position,center);
  let point=|a:f64,side:f64|add(center,add(mul(axle,side*0.035),add([0.,a.sin()*0.09,0.],mul(radial,a.cos()*0.09))));
  for i in 0..12 {let a=i as f64*std::f64::consts::TAU/12.+self.wheel_angle;let b=a+std::f64::consts::TAU/12.;
   for triangle in [[point(a,-1.),point(b,-1.),point(b,1.)],[point(a,-1.),point(b,1.),point(a,1.)], [add(center,mul(axle,-0.035)),point(b,-1.),point(a,-1.)],[add(center,mul(axle,0.035)),point(a,1.),point(b,1.)]] {
    for p in triangle {self.vertices.push([p[0]as f32,p[1]as f32,p[2]as f32,0.035,0.035,0.04,0.]);}
   }
  }
 }
 fn rebuild(&mut self){
  self.vertices.clear();let h=self.height;let fabric=[0.16,0.035,0.045];let metal=[0.32,0.34,0.37];let dark=[0.07,0.075,0.08];
  self.box_part([0.,h,0.],[0.68,0.16,0.64],fabric,0.);
  self.box_part([0.,h+0.46,-0.29],[0.64,0.83,0.12],fabric,0.);
  self.box_part([0.,0.32,0.],[0.14,0.42,0.14],dark,0.);
  self.box_part([0.,(h+0.28)*0.5,0.],[0.075,h-0.28,0.075],metal,0.);
  for x in [-0.39,0.39]{self.box_part([x,h+0.15,0.],[0.045,0.36,0.045],metal,0.);self.box_part([x,h+0.34,0.],[0.11,0.06,0.48],dark,0.);}
  for i in 0..5 {let a=i as f64*72.;let(s,c)=a.to_radians().sin_cos();
   self.box_part([s*0.24,0.22,c*0.24],[0.075,0.07,0.52],metal,a);
   self.box_part([s*0.48,0.15,c*0.48],[0.07,0.16,0.07],metal,a);
   self.wheel([s*0.48,0.09,c*0.48],a);
  }
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn office_chair_height_and_mesh_stay_bounded(){let mut c=Chair::default();let n=c.vertices.len();for _ in 0..100{c.adjust(10.);}assert_eq!(c.height,1.45);for _ in 0..100{c.adjust(-10.);}assert_eq!(c.height,0.65);assert_eq!(n,c.vertices.len());assert!(c.vertices.iter().all(|p|p.iter().all(|v|v.is_finite())));}
 #[test]fn office_chair_rolls_and_stops_at_deck_edge(){let v=load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();let mut c=Chair::default();let before=c.position;let p=add(c.position,[0.,2.,-0.9]);c.push(p,add(p,[0.,0.,0.25]),2.,&v);assert!(c.position[2]>before[2]);c.position=[6.25,6.,-51.];c.rebuild();let p=add(c.position,[-0.9,2.,0.]);let q=c.push(p,add(p,[0.3,0.,0.]),2.,&v);assert_eq!(c.position[0],6.25);assert!(q[0]<=c.position[0]-0.819);}
}
