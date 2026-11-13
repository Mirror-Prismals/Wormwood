//! Original DawFaderSystem display feedback on the existing pass-through text
//! surface. It never participates in input ownership or gem appearance keys.
use serde_json::{Value,json};
fn rect(v:&Value)->Option<[f64;4]>{let a=v.as_array()?;if a.len()!=4{return None;}let mut r=[0.;4];for i in 0..4{r[i]=a[i].as_f64()?;if !r[i].is_finite(){return None;}}(r[2]>0.&&r[3]>0.).then_some(r)}
fn label(labels:&mut Vec<Value>,text:impl AsRef<str>,r:[f64;4],size:f64,center:bool,color:[f64;3]){if labels.len()<256&&r[2]>0.&&r[3]>0.{labels.push(json!({"text":super::plugin_surface::label_text(text.as_ref(),1024),"rect":r,"size":size,"center":center,"color":color}));}}
fn hz(v:f64)->String{if v>=1000.{format!("{:.1} kHz",v/1000.)}else{format!("{:.0} Hz",v.max(0.))}}
fn norm_hz(v:f64,rate:f64)->f64{if v<=0.001{0.}else{30.*(rate.max(60.)*0.5/30.).powf(v.clamp(0.,1.))}}
pub(crate) fn surface(packet:&Value,pointer:Option<[f64;2]>)->Value{
 let Some(panel)=rect(&packet["panel"])else{return Value::Null;};let mut labels=Vec::new();let mut faces=Vec::new();
 let dark=[30.6;3];let light=[239.7,244.8,255.];
 let page=packet["page"].as_u64().unwrap_or(0);
 if page==0{
  if let (Some(content),Some(controls))=(rect(&packet["content"]),packet["controls"].as_array()){
   for c in controls {if !c["id"].as_str().is_some_and(|s|s.starts_with("Fader(")){continue;}if let Some(r)=rect(&c["rect"]){label(&mut labels,c["label"].as_str().unwrap_or(""),[r[0]+r[2]*0.5-45.,content[1]-22.,90.,20.],13.,true,light);}}
   for c in controls.iter().filter(|c|c["active"]==true&&c["id"].as_str().is_some_and(|id|id.starts_with("SelectBus("))){if let Some(r)=rect(&c["rect"]){for edge in [[r[0]-2.,r[1]-2.,r[2]+4.,2.],[r[0]-2.,r[1]+r[3],r[2]+4.,2.]]{faces.push(json!({"rect":edge,"radius":0.,"alpha":1.,"color":[239.7,244.8,255.]}));}}}
   if let Some(meters)=packet["master_meters"].as_array(){for m in meters{if let Some(r)=rect(&m["rect"]){label(&mut labels,m["label"].as_str().unwrap_or(""),[r[0]+r[2]*0.5-17.,content[1]+content[3]-1.,34.,22.],11.,true,dark);}}}
  }
 }
 if page==3{if let Some(outer)=rect(&packet["view"]){
  let area=[outer[0]+8.,outer[1]+8.,(outer[2]-16.).max(1.),(outer[3]-16.).max(1.)];let preview=&packet["preview"];
  if preview["id"].is_null(){label(&mut labels,"SELECT AN AUDIO CLIP TO EDIT",[outer[0],outer[1]+outer[3]*0.5-10.,outer[2],20.],15.,true,light);}
  else {
   let frames=preview["clip_frames"].as_f64().unwrap_or(0.);let rate=preview["sample_rate"].as_f64().unwrap_or(48000.);let seconds=frames/rate;
   let line=|dy:f64|[area[0]+8.,area[1]+dy-7.,(area[2]-16.).max(1.),14.];
   match packet["editor_tab"].as_str().unwrap_or(""){
    "Waveform" | "Warp"=>{
     let warp_tab=packet["editor_tab"]=="Warp";
     let span=packet["wave_view"].as_array();let lo=if warp_tab {0.}else{span.and_then(|a|a.first()).and_then(Value::as_f64).unwrap_or(0.)};let hi=if warp_tab {1.}else{span.and_then(|a|a.get(1)).and_then(Value::as_f64).unwrap_or(1.)};let width=(hi-lo).max(f64::EPSILON);
     let mut face=|r:[f64;4],color:[f64;3],alpha:f64|{faces.push(json!({"rect":r,"radius":0.,"alpha":alpha,"color":color}));};
     // Unlit waveform and bars preserve the persistent baked panel underneath.
     if let Some(wave)=preview["waveform"].as_array().filter(|a|!a.is_empty()) {let columns=area[2].ceil().clamp(1.,2048.)as usize;for i in 0..columns{let source_lo=preview["wave_view"][0].as_f64().unwrap_or(0.);let source_hi=preview["wave_view"][1].as_f64().unwrap_or(1.);let source_width=(source_hi-source_lo).max(f64::EPSILON);let a=((lo+i as f64/columns as f64*width-source_lo)/source_width).clamp(0.,1.);let b=((lo+(i+1)as f64/columns as f64*width-source_lo)/source_width).clamp(0.,1.);let first=(a*wave.len()as f64).floor()as usize;let last=((b*wave.len()as f64).ceil()as usize).max(first+1).min(wave.len());let mut low=1f64;let mut high= -1f64;for v in &wave[first.min(wave.len())..last]{low=low.min(v[0].as_f64().unwrap_or(0.));high=high.max(v[1].as_f64().unwrap_or(0.));}let y=area[1]+area[3]*0.5;face([area[0]+i as f64/columns as f64*area[2],y-high*area[3]*0.44,(area[2]/columns as f64).max(0.5),((high-low)*area[3]*0.44).max(1.)],[165.,221.,232.],0.95);}}
     if let Some(a)=packet["wave_selection"].as_array(){let a0=a[0].as_f64().unwrap_or(0.);let a1=a[1].as_f64().unwrap_or(0.);let x0=((a0.min(a1)-lo)/width).clamp(0.,1.);let x1=((a0.max(a1)-lo)/width).clamp(0.,1.);face([area[0]+x0*area[2],area[1],(x1-x0)*area[2],area[3]],[89.25,209.1,242.25],0.16);}
     if !warp_tab {
      label(&mut labels,"ZOOM",[outer[0]+5.,outer[1]-43.,outer[2]-10.,16.],10.,false,light);
      label(&mut labels,"LOOP",[outer[0]+5.,outer[1]-21.,outer[2]-10.,16.],10.,false,light);
     }
     for i in 0..=4{let f=i as f64/4.;label(&mut labels,format!("{:.6}s",(lo+f*width)*seconds),[area[0]+f*(area[2]-55.),area[1]+2.,55.,14.],10.,false,light);}
     if warp_tab {
     let entry=preview["warp"].as_array().and_then(|a|a.iter().find(|e|e["id"]==preview["id"]));
     let text=entry.map(|e|format!("WARP {}  /  {} MARKERS  /  TRANSIENTS SHOWN THIN",if e["enabled"]==true{"ENABLED"}else{"DISABLED"},e["markers"].as_array().map_or(0,Vec::len))).unwrap_or_else(||"WARP MAP NOT CREATED YET  /  TOGGLE WARP TO CREATE IT".into());
     label(&mut labels,text,[area[0],area[1]+18.,area[2],14.],11.,false,light);
     let start=preview["source_start_frame"].as_f64().unwrap_or(0.);
     if let Some(marks)=preview["transients"].as_array(){for m in marks{let f=m["source_frame"].as_f64().unwrap_or(-1.);if f<start||f>start+frames||frames<=0.{continue;}let x=area[0]+(f-start)/frames*area[2];let alpha=m["strength"].as_f64().unwrap_or(0.).clamp(0.15,1.);if faces.len()<8192{faces.push(json!({"rect":[x-0.5,area[1],1.,area[3]],"radius":0.,"alpha":alpha,"color":[89.25,209.1,242.25]}));}}}
     if let Some(markers)=entry.and_then(|e|e["markers"].as_array()) {for (i,m) in markers.iter().enumerate(){
      let f=m["source_frame"].as_f64().unwrap_or(-1.);if f<start||f>start+frames||frames<=0.{continue;}
      let x=area[0]+(f-start)/frames*area[2];let selected=packet["warp_selected"].as_u64()==Some(i as u64);let color=if selected{light}else{[242.25,191.25,76.5]};
      faces.push(json!({"rect":[x-1.,area[1],2.,area[3]],"radius":0.,"alpha":1.,"color":color}));
      faces.push(json!({"rect":[x-5.,area[1],10.,12.],"radius":2.,"alpha":1.,"color":color}));
      label(&mut labels,if i==0||i+1==markers.len(){"FIXED".to_owned()}else{format!("{}",i)},[x-20.,area[1]+32.,40.,14.],10.,true,color);
     }}
     }
    },
    "Spectral"=>{
     label(&mut labels,"DRAG A TIME / FREQUENCY REGION, THEN ATTENUATE  /  DISPLAY UPDATES FROM PROCESSED AUDIO",line(13.),10.,false,light);
     if let Some(s)=packet["spectral_selection"].as_array().filter(|a|a.len()==4){let n=|i:usize|s[i].as_f64().unwrap_or(0.);label(&mut labels,format!("SELECT {:.2}s  /  {} - {}  /  {:.0} dB",(n(1)-n(0)).abs()*seconds,hz(norm_hz(n(2).min(n(3)),rate)),hz(norm_hz(n(2).max(n(3)),rate)),packet["attenuation_db"].as_f64().unwrap_or(0.)),line(27.),10.,false,light);}
     if let Some([x,y])=pointer.filter(|p|p.iter().all(|v|v.is_finite())&&p[0]>=area[0]&&p[0]<=area[0]+area[2]&&p[1]>=area[1]&&p[1]<=area[1]+area[3]){
      for r in [[x-0.45,area[1],0.9,area[3]],[area[0],y-0.45,area[2],0.9]]{faces.push(json!({"rect":r,"radius":0.,"alpha":1.,"color":[234.6,244.8,255.]}));}
      label(&mut labels,format!("{:.2}s  /  {}",(x-area[0])/area[2]*seconds,hz(norm_hz(1.-(y-area[1])/area[3],rate))),[(x+8.).min(area[0]+area[2]-150.).max(area[0]),(y-14.).max(area[1]+6.),150f64.min(area[2]),14.],10.,false,light);
     }
    },_=>{}
   }
  }
 }}
 if let Some(notice)=packet["notice"].as_str(){label(&mut labels,notice,[panel[0]+8.,panel[1]+panel[3]-22.,(panel[2]-16.).max(1.),18.],11.,false,light);}
 if labels.is_empty()&&faces.is_empty(){Value::Null}else{json!({"clip":panel,"labels":labels,"browser_faces":faces})}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn empty_editor_and_visible_notices_use_original_prompt(){let p=json!({"page":3,"panel":[0.,0.,800.,300.],"view":[120.,80.,650.,190.],"notice":"Microphone service unavailable"});let s=surface(&p,None);assert!(super::super::plugin_surface::valid(&json!({"groups":[s.clone()]})));assert_eq!(s["labels"][0]["text"],"SELECT AN AUDIO CLIP TO EDIT");assert_eq!(s["labels"].as_array().unwrap().len(),2);}
 #[test]fn warp_overlay_shows_source_and_draggable_markers_without_a_background(){
  let p=json!({"page":3,"panel":[0.,0.,800.,300.],"view":[120.,80.,650.,190.],"editor_tab":"Warp","warp_selected":1,"preview":{"id":"c","clip_frames":48000,"sample_rate":48000,"source_start_frame":1000,"waveform":[[-0.5,0.5]],"warp":[{"id":"c","enabled":true,"markers":[{"source_frame":1000},{"source_frame":25000},{"source_frame":49000}]}]}});
  let s=surface(&p,None);let faces=s["browser_faces"].as_array().unwrap();
  assert!(faces.iter().any(|f|f["rect"]==json!([444.,88.,2.,174.]) && f["color"]==json!([239.7,244.8,255.])));
  assert_eq!(faces.iter().filter(|f|f["rect"][2]==10. && f["rect"][3]==12.).count(),3);
  assert!(!faces.iter().any(|f|f["rect"]==json!([120.,80.,650.,190.])));
  assert!(super::super::plugin_surface::valid(&json!({"groups":[s]})));
 }
 #[test]fn spectral_hover_is_live_and_never_mutates_the_optical_packet(){let p=json!({"page":3,"panel":[0.,0.,800.,300.],"view":[120.,80.,650.,190.],"editor_tab":"Spectral","preview":{"id":"c","clip_frames":48000,"sample_rate":48000}});let before=p.clone();let a=surface(&p,None);let b=surface(&p,Some([300.,150.]));assert_eq!(p,before);assert_eq!(b["browser_faces"].as_array().unwrap().len(),2);assert_ne!(a,b);assert!(super::super::plugin_surface::valid(&json!({"groups":[b]})));}
}
