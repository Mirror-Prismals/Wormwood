//! Optional exact independence certificate for the two moving timeline markers.
//! No hit is skipped or changed. Unproved/aliased samples retain canonical RGB.
const MARKER:&str="// METER_LEDGER_PLAYBACK_CERTIFICATE";
fn once(s:&mut String,old:&str,new:&str)->Result<(),String>{
 if s.matches(old).count()!=1{return Err(format!("meter playback marker changed: {old}"));}
 *s=s.replacen(old,new,1);Ok(())
}
fn function<'a>(s:&'a str,name:&str)->Result<&'a str,String>{
 let start=s.find(&format!("fn {name}(" )).ok_or_else(||format!("missing {name}"))?;
 let open=start+s[start..].find('{').ok_or("missing function body")?;let mut depth=0;
 for(i,b)in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok(&s[start..=i]);}}}
 Err("unclosed function".into())
}
pub(super) fn source(input:&str)->Result<String,String>{
 if input.contains(MARKER)||!input.contains("fn cs_meter_ledger_capture("){return Err("meter playback requires one production ledger".into());}
 // All optical packet reads must pass the audited accessor. Dynamic material,
 // geometry or glyph addresses could otherwise alias one of the exempt lanes.
 if input.matches("workstationData.rows[").count()!=1{return Err("untracked direct optical packet read".into());}
 let mut s=input.to_string();
 once(&mut s,"fn gemRow(index:i32)->vec4<f32>{return workstationData.rows[u32(index)];}",r#"fn gemRow(index:i32)->vec4<f32>{
 if(meterLedgerPlaybackCapture&&(index==245||index==252||index==45751)){meterLedgerRejected=true;}
 return workstationData.rows[u32(index)];
}"#)?;
 // Only scalar x/z reads bypass alias tracking. Direct clock.w stays tracked,
 // even though the CPU admission guard requires both custom paths disabled.
 for lane in ["x","z"] {
  let old=format!("gemRow(245).{lane}");let new=format!("meterLedgerStableStyleHeader().{lane}");
  if !s.contains(&old){return Err("style header guard source changed".into());}s=s.replace(&old,&new);
 }
 let old=function(&s,"gemTimelinePlayhead")?.to_string();let mut checked=old.clone();
 if checked.matches("mark.x").count()!=1||checked.matches("control.w").count()!=1{
  return Err("moving marker coordinate has an unaudited consumer".into());
 }
 once(&mut checked,"var mark:vec4<f32>=gemRow(252);","var mark:vec4<f32>=meterLedgerKnownMarker();")?;
 once(&mut checked,"var control:vec4<f32>=gemRow(barBase+7);","var control:vec4<f32>=meterLedgerKnownHandle();")?;
 once(&mut checked,"        hit=gemTimelineBackdropMarkerBox(origin,ray,center,extent,hit.distance);",r#"        meterLedgerDynamicBox=true;
        hit=gemTimelineBackdropMarkerBox(origin,ray,center,extent,hit.distance);
        meterLedgerDynamicBox=false;"#)?;
 once(&mut checked,"            var grab:GemHit=gemTimelineBackdropMarkerBox(origin,ray,vec3<f32>(control.w,control.y,0.020),vec3<f32>(control.z,control.z,0.001),hit.distance);",r#"            meterLedgerDynamicBox=true;
            var grab:GemHit=gemTimelineBackdropMarkerBox(origin,ray,vec3<f32>(control.w,control.y,0.020),vec3<f32>(control.z,control.z,0.001),hit.distance);
            meterLedgerDynamicBox=false;"#)?;
 once(&mut s,&old,&checked)?;
 let old=function(&s,"gemTimelineMarkerBox")?.to_string();let mut checked=old.clone();
 let arithmetic="    var lo:vec3<f32>=min(a,b);var hi:vec3<f32>=max(a,b);var near:f32=max(max(lo.x,lo.y),lo.z);var far:f32=min(min(hi.x,hi.y),hi.z);";
 once(&mut checked,arithmetic,&format!("{arithmetic}\n{}",r#"    if(meterLedgerPlaybackCapture&&meterLedgerDynamicBox){
        // Use the ORIGINAL evaluated a/b/lo/hi, not another intersection or a
        // guessed world-space bound. These inequalities contradict the exact
        // original hit predicate for EVERY finite X interval.
        let lower=max(lo.y,lo.z);let upper=min(hi.y,hi.z);
        let bounded=meterLedgerFinite(origin)&&meterLedgerFinite(ray)&&meterLedgerFinite(center)&&meterLedgerFinite(extent)
            &&meterLedgerFinite(a)&&meterLedgerFinite(b)&&meterLedgerFinite(vec3<f32>(farLimit))
            &&all(abs(origin)<=vec3<f32>(1000000.0))&&all(abs(ray)<=vec3<f32>(1000000.0))
            &&all(abs(center)<=vec3<f32>(1000000.0))&&all(extent>=vec3<f32>(0.0))&&all(extent<=vec3<f32>(1000000.0));
        if(!bounded||!(upper<lower||lower>=farLimit||upper<=0.0000001)){meterLedgerRejected=true;}
    }"#))?;
 once(&mut s,&old,&checked)?;
 once(&mut s," meterLedgerIndex=index;meterLedgerCount=0u;meterLedgerOverflow=false;meterLedgerRejected=false;",
    " meterLedgerIndex=index;meterLedgerCount=0u;meterLedgerOverflow=false;meterLedgerRejected=false;\n meterLedgerPlaybackCapture=true;meterLedgerDynamicBox=false;")?;
 once(&mut s," let captured=meterLedgerCaptureTrace(origin,direction,random);",
    " let captured=meterLedgerCaptureTrace(origin,direction,random);\n meterLedgerPlaybackCapture=false;meterLedgerDynamicBox=false;")?;
 s.push_str(r#"
// METER_LEDGER_PLAYBACK_CERTIFICATE
var<private> meterLedgerPlaybackCapture:bool=false;
var<private> meterLedgerDynamicBox:bool=false;
fn meterLedgerStableStyleHeader()->vec4<f32>{return workstationData.rows[245u];}
fn meterLedgerKnownMarker()->vec4<f32>{return workstationData.rows[252u];}
fn meterLedgerKnownHandle()->vec4<f32>{return workstationData.rows[45751u];}
"#);
 Ok(s)
}

#[cfg(test)] #[path="WorkstationMeterLedgerPlaybackProof.rs"] mod proof;
#[cfg(test)] mod tests {
 use super::*;
 fn base()->String{super::super::source_with_playhead(&super::super::tests::base(),false).unwrap()}
 #[test]fn source_rejects_unaudited_paths(){
  let original=base();let s=original.as_str();
  assert!(source(&(s.to_string()+"\nfn illicit()->vec4<f32>{return workstationData.rows[252u];}")).is_err());
  assert!(source(&s.replace("var pixel:f32=gemRow(253).x;","var pixel:f32=gemRow(253).x+mark.x;")).is_err());
  assert!(source(&s.replace("var control:vec4<f32>=gemRow(barBase+7);","var control:vec4<f32>=gemRow(barBase+6);")).is_err());
 }
 #[test]fn original_intersection_arithmetic_and_trace_remain_verbatim(){
  let original=base();let s=original.as_str();let out=source(s).unwrap();
  for name in ["retainedMeterTrace","retainedMeterTerminal","gemTerminal","gemNearest","meterLedgerCaptureTrace","meterLedgerEvaluate"]{
   // Clock.x/z guard reads are intentionally rerouted to a raw immutable-lane
   // accessor. No numerical expression in these functions is otherwise edited.
   let expected=function(s,name).unwrap().replace("gemRow(245).x","meterLedgerStableStyleHeader().x").replace("gemRow(245).z","meterLedgerStableStyleHeader().z");
   assert_eq!(function(&out,name).unwrap(),expected,"{name}");
  }
  let old=function(s,"gemTimelineMarkerBox").unwrap();let new=function(&out,"gemTimelineMarkerBox").unwrap();
  for line in old.lines().filter(|l|l.contains("var a:")||l.contains("var lo:")||l.contains("if(near>")||l.contains("normal=")||l.contains("hit=GemHit")) {assert!(new.contains(line),"{line}");}
  assert_eq!(out.matches("meterLedgerDynamicBox=true;").count(),2);
  assert_eq!(out.matches("meterLedgerPlaybackCapture=true;").count(),1);
  assert!(function(&out,"gemRow").unwrap().contains("index==245||index==252||index==45751"));
  assert!(out.contains("gemRow(245).w")); // actual clock consumers never bypass guard
 }
 #[test]fn complete_shader_validates_and_does_not_add_bindings(){
  let original=base();let s=original.as_str();let out=source(s).unwrap();
  let module=naga::front::wgsl::parse_str(&out).unwrap_or_else(|e|panic!("{}",e.emit_to_string(&out)));
  let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
  for(index,entry)in module.entry_points.iter().enumerate(){
   let expected=match entry.name.as_str(){"cs_meter_ledger_capture"=>vec![1,6,9,10],"fs_retained"=>vec![1,5,6,9,13],_=>continue};
   let usage=info.get_entry_point(index);let mut used=module.global_variables.iter().filter_map(|(h,v)|if usage[h].is_empty(){None}else{v.binding.as_ref().map(|b|b.binding)}).collect::<Vec<_>>();used.sort();assert_eq!(used,expected);
  }
 }
}
