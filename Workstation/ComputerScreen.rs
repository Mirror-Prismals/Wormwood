//! Physical theatre display: one perspective map shared by image and input.
use serde_json::{Value,json};
fn v(view:&Value,key:&str)->[f64;3]{std::array::from_fn(|i|view[key][i].as_f64().unwrap_or(0.))}
fn dot(a:[f64;3],b:[f64;3])->f64{a.iter().zip(b).map(|(a,b)|a*b).sum()}
// Authored glass at local (0,.175,.055), rotated 180 degrees by the venue.
// The extra millimetre puts the unlit display in front of the glass.
pub(crate) fn map(view:&Value,computer:[f64;3],aspect:f64)->[[f32;4];3]{
 let eye=v(view,"position");let center=[computer[0]-eye[0],computer[1]+0.175-eye[1],computer[2]-0.059-eye[2]];
 let x=[-0.443,0.,0.];let y=[0.,0.255,0.];let f=1./(70f64.to_radians()*0.5).tan();
 std::array::from_fn(|i|{let basis=v(view,["right","up","forward"][i]);let scale=if i==0{f/aspect}else if i==1{f}else{1.};
 [ (dot(x,basis)*scale) as f32,(dot(y,basis)*scale) as f32,(dot(center,basis)*scale) as f32,1.]})
}
/// Forward projection for a pass-through drag sprite above the physical display.
pub(crate) fn project(m:[[f32;4];3],p:[f64;2],size:[f64;2])->Option<[f64;2]>{
 if size.iter().any(|x|!x.is_finite()||*x<=0.)||p.iter().any(|x|!x.is_finite()){return None;}
 let x=p[0]*2./size[0]-1.;let y=1.-p[1]*2./size[1];
 let h=|i:usize|m[i][0] as f64*x+m[i][1] as f64*y+m[i][2] as f64;
 let z=h(2);if z<=0.||!z.is_finite(){return None;}
 Some([(h(0)/z+1.)*size[0]*0.5,(1.-h(1)/z)*size[1]*0.5])
}
pub(crate) fn pointer(m:[[f32;4];3],p:[f64;2],size:[f64;2])->Option<[f64;2]>{pointer_for_drag(m,p,size,false)}
fn pointer_for_drag(m:[[f32;4];3],p:[f64;2],size:[f64;2],captured:bool)->Option<[f64;2]>{
 if size.iter().any(|x|!x.is_finite()||*x<=0.)||p.iter().any(|x|!x.is_finite()){return None;}
 let sx=p[0]*2./size[0]-1.;let sy=1.-p[1]*2./size[1];
 let a=m[0][0] as f64-sx*m[2][0] as f64;let b=m[0][1] as f64-sx*m[2][1] as f64;
 let c=m[1][0] as f64-sy*m[2][0] as f64;let d=m[1][1] as f64-sy*m[2][1] as f64;
 let e=sx*m[2][2] as f64-m[0][2] as f64;let f=sy*m[2][2] as f64-m[1][2] as f64;let det=a*d-b*c;
 if det.abs()<1e-10{return None;}let x=(e*d-b*f)/det;let y=(a*f-e*c)/det;
 if (!captured&&(x.abs()>1.||y.abs()>1.))||m[2][0] as f64*x+m[2][1] as f64*y+m[2][2] as f64<=0.{return None;}
 Some([(x+1.)*size[0]*0.5,(1.-y)*size[1]*0.5])
}
pub(crate) fn input(value:&mut Value,m:[[f32;4];3],size:[f64;2]){input_for_drag(value,m,size,false)}
pub(crate) fn input_for_drag(value:&mut Value,m:[[f32;4];3],size:[f64;2],captured:bool){
 match value{Value::Object(o)=>{for(k,v)in o.iter_mut(){if k=="pointer"&&v.is_array(){let p=serde_json::from_value::<[f64;2]>(v.clone()).ok();*v=p.and_then(|p|pointer_for_drag(m,p,size,captured)).map_or(Value::Null,|p|json!(p));}else{input_for_drag(v,m,size,captured);}}},Value::Array(a)=>for v in a{input_for_drag(v,m,size,captured)},_=>{}}
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn captured_drag_extrapolates_past_display_but_hover_does_not(){
  let m=[[1.,0.,0.,0.],[0.,1.,0.,0.],[0.,0.,1.,0.]];let size=[1000.,800.];
  let mut sample=json!({"pointer":[-240.,900.],"left":true,"transport_pointer_events":[{"pointer":[-240.,900.],"left":false}]});
  assert!(pointer(m,[-240.,900.],size).is_none());
  input_for_drag(&mut sample,m,size,true);
  assert!((sample["pointer"][0].as_f64().unwrap()+240.).abs()<1e-9);
  assert!((sample["pointer"][1].as_f64().unwrap()-900.).abs()<1e-9);
  assert_eq!(sample["transport_pointer_events"][0]["pointer"],sample["pointer"]);
 }

 #[test]fn oblique_computer_projection_round_trips(){
 let standing=json!({"position":[0.5,8.6,-49.],"forward":[0.,0.,1.],"right":[-1.,0.,0.],"up":[0.,1.,0.]});
 let m=map(&standing,[0.,8.,-47.],1.6);
 let p=project(m,[640.,400.],[1280.,800.]).unwrap();
 let q=pointer(m,p,[1280.,800.]).unwrap();
 assert!((q[0]-640.).abs()<0.001 && (q[1]-400.).abs()<0.001);
 }
 #[test]fn physical_click_events_share_the_hover_projection(){
 let m=[[1.,0.,0.,1.],[0.,1.,0.,1.],[0.,0.,2.,1.]];
 let mut input_value=json!({"pointer":[600.,350.],"left":true,"transport_pointer_events":[{"pointer":[600.,350.],"left":true},{"pointer":[0.,0.],"left":false}]});
 input(&mut input_value,m,[1000.,800.]);
 let expected=pointer(m,[600.,350.],[1000.,800.]).unwrap();
 assert_eq!(input_value["pointer"],json!(expected));
 assert_eq!(input_value["transport_pointer_events"][0]["pointer"],json!(expected));
 assert!(input_value["transport_pointer_events"][1]["pointer"].is_null());
 assert_eq!(input_value["transport_pointer_events"][1]["left"],false);
 }

 #[test]fn perspective_image_and_pointer_agree(){let view=json!({"position":[0.,8.,-49.],"right":[-1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,1.]});let m=map(&view,[0.,8.,-47.],1.6);let size=[1280.,800.];
 for(x,y)in [(0.,0.),(-0.8,0.7),(0.9,-0.9)]{let h=|i:usize|m[i][0] as f64*x+m[i][1] as f64*y+m[i][2] as f64;let p=[(h(0)/h(2)+1.)*size[0]/2.,(1.-h(1)/h(2))*size[1]/2.];assert!(project(m,[(x+1.)*size[0]/2.,(1.-y)*size[1]/2.],size).unwrap().iter().zip(p).all(|(a,b)|(*a-b).abs()<0.001));let q=pointer(m,p,size).unwrap();assert!((q[0]-(x+1.)*size[0]/2.).abs()<0.001);assert!((q[1]-(1.-y)*size[1]/2.).abs()<0.001);}
 assert!(pointer(m,[0.,0.],size).is_none());}
}
