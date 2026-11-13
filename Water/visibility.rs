//! Conservative presentation visibility. Never used for water occupancy.
use super::math::*;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(crate) struct View { pub eye:V, planes:[V;5] }
impl View {
    pub fn parse(v:&Value)->Option<Self>{
        let vector=|key:&str|->Option<V>{let a=v[key].as_array()?;if a.len()!=3{return None;}let p=[a[0].as_f64()?,a[1].as_f64()?,a[2].as_f64()?];p.iter().all(|n|n.is_finite()).then_some(p)};
        let eye=vector("position")?;let right=vector("right")?;let up=vector("up")?;let forward=vector("forward")?;
        let aspect=v["aspect"].as_f64()?;
        if !aspect.is_finite()||!(0.2..=8.).contains(&aspect)
            ||[right,up,forward].iter().any(|p|(len(*p)-1.).abs()>1e-5)
            ||dot(right,up).abs()>1e-5||dot(right,forward).abs()>1e-5||dot(up,forward).abs()>1e-5{return None;}
        // Same 70 degree vertical FOV as the opaque and water native cameras.
        // No far-plane rejection: retain the renderer's full distance policy.
        let y=(70f64.to_radians()*0.5).tan();let x=y*aspect;
        Some(Self{eye,planes:[forward,unit(add(mul(forward,x),right)),unit(sub(mul(forward,x),right)),unit(add(mul(forward,y),up)),unit(sub(mul(forward,y),up))]})
    }
    pub fn sphere(&self,center:V,radius:f64)->bool {
        let p=sub(center,self.eye);
        self.planes.iter().all(|&n|dot(n,p)>=-radius-0.05)
    }
    pub fn triangle(&self,t:[V;3])->bool {
        self.planes.iter().all(|&n|t.iter().any(|&p|dot(n,sub(p,self.eye))>=-0.05))
    }
    pub fn capsule(&self,a:V,b:V,radius:f64)->bool {
        let a=sub(a,self.eye);let b=sub(b,self.eye);
        self.planes.iter().all(|&n|dot(n,a).max(dot(n,b))>=-radius-0.05)
    }
}

/// Global source bounds, independent of camera direction. No sampled height
/// estimate can accidentally exclude a narrow waterfall or a wave crest.
pub(crate) fn height_bounds(deep:bool,b:&super::bodies::State,mean:f64,wave:&super::ocean::State)->Option<[f64;2]>{
    if deep {let [lo,hi]=crate::terrain::depths::RIVER_WATER_RANGE;return Some([lo-16.,hi]);}
    let mut bounds=[f64::INFINITY,f64::NEG_INFINITY];
    let mut include=|lo:f64,hi:f64|{bounds[0]=bounds[0].min(lo);bounds[1]=bounds[1].max(hi);};
    if b.generated {let [lo,hi]=crate::terrain::drainage::water_height_bounds();include(lo,hi);}
    for river in &b.rivers {for pair in river.points.windows(2){
        let d=sub(pair[1],pair[0]);let q=add(pair[0],mul(d,(-dot(pair[0],d)/dot(d,d)).clamp(0.,1.)));
        include(len(q)-crate::terrain::RADIUS,len(pair[0]).max(len(pair[1]))-crate::terrain::RADIUS);
    }}
    if b.ocean {let a=wave.settings.amplitude.max(0.)*wave.settings.shore_gain.max(1.);include(mean-a,mean+a);}
    (bounds[0]<=bounds[1]).then_some(bounds)
}

/// Encloses every radially projected point of a sampling cell at any possible
/// source height, including the underground drop sheet. Normalization is
/// 2/R-Lipschitz on this tangent plane, so the enlarged horizontal radius is a
/// bound rather than a corner-only estimate that can miss spherical curvature.
pub(crate) fn cell_sphere(frame:Frame,x:[f64;2],z:[f64;2],heights:[f64;2])->(V,f64){
    let(a,b,r)=cell_capsule(frame,x,z,heights);
    (mul(add(a,b),0.5),len(sub(a,b))*0.5+r)
}
pub(crate) fn cell_capsule(frame:Frame,x:[f64;2],z:[f64;2],heights:[f64;2])->(V,V,f64){
    let up=unit(frame.world([(x[0]+x[1])*0.5,0.,(z[0]+z[1])*0.5]));
    let half=((x[1]-x[0]).powi(2)+(z[1]-z[0]).powi(2)).sqrt()*0.5;
    let radius=2.*(1.+heights[0].abs().max(heights[1].abs())/crate::terrain::RADIUS)*half+0.05;
    (mul(up,crate::terrain::RADIUS+heights[0]),mul(up,crate::terrain::RADIUS+heights[1]),radius)
}

#[cfg(test)]mod tests {
 use super::*;
 fn view()->View{View::parse(&serde_json::json!({"position":[0.,0.,0.],"right":[1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,-1.],"aspect":1.6})).unwrap()}
 #[test]fn visibility_retains_crossing_edges_and_camera_inside_bounds(){
  let v=view();assert!(!v.sphere([0.,0.,10.],1.));assert!(v.sphere([0.,0.,0.],1.));assert!(v.sphere([0.,0.,-10.],1.));
  assert!(v.triangle([[-100.,0.,-10.],[100.,0.,-10.],[0.,100.,-10.]]));
  assert!(!v.triangle([[10.,0.,-1.],[11.,1.,-1.],[10.,1.,-1.]]));
  assert!(View::parse(&serde_json::json!({})).is_none());
 }
 #[test]fn radial_cell_bounds_enclose_interior_and_height_extrema(){
  for eye in [[0.,crate::terrain::RADIUS,0.],[0.,-crate::terrain::RADIUS,0.],[crate::terrain::RADIUS,0.,0.]]{
   let frame=Frame::radial(eye,[0.,0.,-1.]);
   for x in [[-240.,-234.],[-1.5,0.],[0.,6.],[234.,240.]]{for z in [[-240.,-234.],[0.,1.5],[234.,240.]]{
    let heights=[-77.,120.];let(c,r)=cell_sphere(frame,x,z,heights);let(a,b,cr)=cell_capsule(frame,x,z,heights);
    for i in 0..=8 {for j in 0..=8 {for h in [-77.,-23.,0.,120.]{let p=mul(unit(frame.world([x[0]+(x[1]-x[0])*i as f64/8.,0.,z[0]+(z[1]-z[0])*j as f64/8.])),crate::terrain::RADIUS+h);assert!(len(sub(p,c))<=r);let d=sub(b,a);let q=add(a,mul(d,(dot(sub(p,a),d)/dot(d,d)).clamp(0.,1.)));assert!(len(sub(p,q))<=cr);}}}
   }}
  }
 }
}
