// Registered Host routes with the normal test graphics driver. No native UI.
fn mobile_workflow_step(host:&mut Host,editor:Value)->Value {
 host.invoke("AudioWorkstationSystem.Step",json!({"dt":0.1,"aspect":844./390.,
  "viewport":[844.,390.],"framebuffer":[844.,390.],"canvas_scale":1.,
  "keys":[false,false,false,false,false],"editor":editor})).unwrap()
}
fn mobile_workflow_page(host:&mut Host,name:&str)->Value {
 let mut status=mobile_workflow_step(host,json!({"mobile_panel_request":name,"focused":false,"transport_pointer_cancelled":true}));
 for _ in 0..8 {if status["mobile_animating"]!=true {break;}status=mobile_workflow_step(host,Value::Null);}
 assert_ne!(status["mobile_animating"],true,"bounded transition must reach its destination");status
}
fn mobile_workflow_begin(host:&mut Host){
 host.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();host.audio_workstation.mobile_test_layout();
 mobile_workflow_step(host,Value::Null);
}
#[test]
fn mobile_workflow_single_visible_panel_and_desktop_defaults(){
 let mut h=traced_host();mobile_workflow_begin(&mut h);
 for (i,name) in ["top","bottom","left","right"].into_iter().enumerate(){
  assert_eq!(mobile_workflow_page(&mut h,name)["mobile_page"],name);
  let rows=h.graphics.workstation_frame.as_ref().unwrap();
  for n in 0..4{assert_eq!(rows[32+40*n][0],if i==n{1.}else{0.});}
  assert_eq!(mobile_workflow_page(&mut h,name)["mobile_page"],name,"same swipe cannot skip Timeline");
  let opposite=["bottom","top","right","left"][i];
  let unrelated=if i<2 {"left"}else{"top"};
  assert_eq!(mobile_workflow_page(&mut h,unrelated)["mobile_page"],name,"orthogonal swipe cannot switch panels");
  assert_eq!(mobile_workflow_page(&mut h,opposite)["mobile_page"],"timeline");
  let rows=h.graphics.workstation_frame.as_ref().unwrap();assert!((0..4).all(|n|rows[32+40*n][0]==0.));
 }
 workstation_bubble_close(&mut h);
 h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
 let desktop=workstation_bubble_step(&mut h,Value::Null);assert!(desktop["mobile_page"].is_null());
 assert!(desktop["panels"].as_array().unwrap().iter().all(|p|p["open"]==true));
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]
fn mobile_workflow_component_drag_reveals_timeline_and_inserts_actual_track(){
 use crate::workstation::{panel_motion::{Snapshot,MobilePage,Edge},panel_layout::{Viewport,Options},browser};
 let mut h=traced_host();mobile_workflow_begin(&mut h);mobile_workflow_page(&mut h,"left");
 let view=Viewport{width:844.,height:390.,canvas_scale:1.};
 let motion=Snapshot{mobile_page:Some(MobilePage::Panel(Edge::Left)),..Snapshot::default()};
 let p=browser::layout(view,&motion,Options::default()).unwrap().targets[2].center();
 let s=mobile_workflow_step(&mut h,json!({"pointer":p,"left":true}));assert_eq!(s["browser"]["dragging"],true);
 let s=mobile_workflow_step(&mut h,json!({"pointer":[600.,180.],"left":true}));assert_eq!(s["mobile_page"],"timeline");assert_eq!(s["browser"]["dragging"],true);
 mobile_workflow_step(&mut h,json!({"pointer":[600.,180.],"left":false}));
 let doc=workstation_bubble_snapshot(&mut h);assert_eq!(doc["tracks"].as_array().unwrap().len(),1);assert_eq!(doc["tracks"][0]["kind"],"midi");
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]
fn mobile_workflow_hidden_piano_roll_does_not_disable_transport_or_track_io(){
 use crate::workstation::{panel_motion::{Snapshot,MobilePage,Edge},panel_layout::{self,Viewport,Options}};
 let mut h=traced_host();mobile_workflow_begin(&mut h);workstation_bubble_open_fixture(&mut h);
 mobile_workflow_page(&mut h,"top");
 let view=Viewport{width:844.,height:390.,canvas_scale:1.};
 let motion=Snapshot{mobile_page:Some(MobilePage::Panel(Edge::Top)),..Snapshot::default()};
 let surface=panel_layout::resolved_surface(view,&motion,Options::default(),0).unwrap();
 let mpp=1.3/390.;let scale=(surface.size[0]*0.88/0.205).min(surface.size[1]*0.80/0.04);
 let p=[422.+surface.origin[0]/mpp-1.5*0.055*scale/mpp,195.-surface.origin[1]/mpp];
 let s=mobile_workflow_step(&mut h,json!({"pointer":p,"left":true}));
 assert!(s["transport_pressure"][0].as_f64().unwrap()>0.,"visible transport must capture and physically press");
 mobile_workflow_step(&mut h,json!({"pointer":p,"left":false}));
 mobile_workflow_page(&mut h,"bottom"); // Opposite swipe closes Top before opening Right.
 let s=mobile_workflow_page(&mut h,"right");assert_eq!(s["piano_roll"]["open"],true);
 let control=s["track_io"]["controls"].as_array().unwrap().iter().find(|c|c["key"]=="arm").expect("actual arming control");
 let r=&control["rect"];let p=[r[0].as_f64().unwrap()+r[2].as_f64().unwrap()*0.5,r[1].as_f64().unwrap()+r[3].as_f64().unwrap()*0.5];
 mobile_workflow_step(&mut h,json!({"pointer":p,"left":true}));mobile_workflow_step(&mut h,json!({"pointer":p,"left":false}));
 mobile_workflow_step(&mut h,json!({"pointer":p,"left":false}));
 let doc=workstation_bubble_snapshot(&mut h);assert_eq!(doc["tracks"][0]["arm_mode"],3);
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]
fn mobile_workflow_navigation_cancels_pending_control_without_click_through(){
 let mut h=traced_host();mobile_workflow_begin(&mut h);mobile_workflow_page(&mut h,"left");
 let before=workstation_bubble_snapshot(&mut h);
 mobile_workflow_page(&mut h,"right");mobile_workflow_step(&mut h,json!({"left":false}));
 assert_eq!(workstation_bubble_snapshot(&mut h),before);
 assert_eq!(h.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap()["browser"]["captured"],false);
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
