//! Exact prefix convergence for the wide atlas coverage query and the finite
//! environment query. A distance comparison alone is deliberately insufficient.
fn span(s:&str,name:&str)->Result<(usize,usize),String>{
 let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("outer primary function changed: {name}"));}
 let start=s.find(&marker).unwrap();let open=start+s[start..].find('{').ok_or("missing primary body")?;let mut depth=0;
 for(i,c)in s[open..].char_indices(){if c=='{'{depth+=1;}else if c=='}'{depth-=1;if depth==0{return Ok((start,open+i+1));}}}
 Err("unclosed primary function".into())
}
fn once(s:&mut String,a:&str,b:&str)->Result<(),String>{if s.matches(a).count()!=1{return Err(format!("outer primary marker changed: {a}"));}*s=s.replacen(a,b,1);Ok(())}
const WRAPPER:&str=r#"fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {
 let primary=retainedActive&&medium<0&&all(origin==retainedOrigin);
 retainedPrimary=primary;
 let hit=retainedOriginalNearest(origin,ray,medium,farLimit);
 retainedPrimary=false;
 return hit;
}"#;
const PANEL_END:&str="    var control:GemHit=gemControlHit(origin,ray,-1,hit.distance,false);";
const CONTROL_END:&str="    if(control.panel>=0){hit=control;}";
const CAPTURE_PANELS:&str="    retainedOuterPanels=hit;\n";
const CAPTURE_CONTROLS:&str="\n    retainedOuterControls=hit;";
const CHECK_PANELS:&str="    if(retainedOuterHitEqual(hit,retainedOuterPanels)){return retainedOuterFinal;}\n";
const CHECK_CONTROLS:&str="\n    if(retainedOuterHitEqual(hit,retainedOuterControls)){return retainedOuterFinal;}";
const OUTER_CALL:&str="let h=gemNearest(origin,direction,-1,1e20);";
const NEW_OUTER_CALL:&str="let h=retainedOuterBegin(origin,direction);";
const NEW_WRAPPER:&str=r#"fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {
 let primary=retainedActive&&medium<0&&all(origin==retainedOrigin);
 retainedPrimary=primary;
 var hit:GemHit;
 if(retainedOuterAvailable&&primary&&medium== -1&&retainedGroup==retainedOuterGroup&&retainedOuterFinite(farLimit)
    &&all(bitcast<vec3<u32>>(origin)==bitcast<vec3<u32>>(retainedOuterOrigin))
    &&all(bitcast<vec3<u32>>(ray)==bitcast<vec3<u32>>(retainedOuterDirection))){
  if(bitcast<u32>(farLimit)==bitcast<u32>(1e20)){hit=retainedOuterFinal;}
  else{hit=retainedOuterNarrow(origin,ray,medium,farLimit);}
 }else{hit=retainedOriginalNearest(origin,ray,medium,farLimit);}
 retainedPrimary=false;
 return hit;
}"#;
const SUPPORT:&str=r#"
var<private> retainedOuterAvailable:bool=false;
var<private> retainedOuterOrigin:vec3<f32>;
var<private> retainedOuterDirection:vec3<f32>;
var<private> retainedOuterGroup:i32;
var<private> retainedOuterPanels:GemHit;
var<private> retainedOuterControls:GemHit;
var<private> retainedOuterFinal:GemHit;
fn retainedOuterFinite(value:f32)->bool{return (bitcast<u32>(value)&0x7f800000u)!=0x7f800000u;}
fn retainedOuterHitFinite(hit:GemHit)->bool{
 return retainedOuterFinite(hit.distance)&&retainedOuterFinite(hit.normal.x)
     &&retainedOuterFinite(hit.normal.y)&&retainedOuterFinite(hit.normal.z);
}
fn retainedOuterHitEqual(a:GemHit,b:GemHit)->bool{
 return retainedOuterHitFinite(a)&&retainedOuterHitFinite(b)&&a.panel==b.panel
     &&all(bitcast<vec4<u32>>(vec4<f32>(a.distance,a.normal))==bitcast<vec4<u32>>(vec4<f32>(b.distance,b.normal)));
}
fn retainedOuterBegin(origin:vec3<f32>,direction:vec3<f32>)->GemHit{
 retainedOuterAvailable=false;
 let primary=retainedActive&&all(origin==retainedOrigin);
 retainedPrimary=primary;
 let hit=retainedOuterCapture(origin,direction,-1,1e20);
 retainedPrimary=false;
 retainedOuterOrigin=origin;retainedOuterDirection=direction;retainedOuterGroup=retainedGroup;retainedOuterFinal=hit;
 retainedOuterAvailable=primary&&retainedOuterHitFinite(hit)
     &&all((bitcast<vec3<u32>>(origin)&vec3<u32>(0x7f800000u))!=vec3<u32>(0x7f800000u))
     &&all((bitcast<vec3<u32>>(direction)&vec3<u32>(0x7f800000u))!=vec3<u32>(0x7f800000u));
 return hit;
}
"#;
pub(super)fn source(original:&str)->Result<String,String>{
 if original.contains("fn retainedOuterBegin("){return Err("outer primary already installed".into());}
 let(a,b)=span(original,"gemNearest")?;if &original[a..b]!=WRAPPER{return Err("retained primary wrapper changed".into());}
 let(a,b)=span(original,"retainedOriginalNearest")?;let reference=&original[a..b];
 let mut capture=reference.to_string();once(&mut capture,"fn retainedOriginalNearest(","fn retainedOuterCapture(")?;
 once(&mut capture,PANEL_END,&format!("{CAPTURE_PANELS}{PANEL_END}"))?;
 once(&mut capture,CONTROL_END,&format!("{CONTROL_END}{CAPTURE_CONTROLS}"))?;
 let mut narrow=reference.to_string();once(&mut narrow,"fn retainedOriginalNearest(","fn retainedOuterNarrow(")?;
 once(&mut narrow,PANEL_END,&format!("{CHECK_PANELS}{PANEL_END}"))?;
 once(&mut narrow,CONTROL_END,&format!("{CONTROL_END}{CHECK_CONTROLS}"))?;
 let mut result=original.to_string();once(&mut result,WRAPPER,NEW_WRAPPER)?;
 let(a,b)=span(&result,"cs_retained")?;let entry=&result[a..b];if entry.matches(OUTER_CALL).count()!=2{return Err("capture query layout changed".into());}
 let entry=entry.replace(OUTER_CALL,NEW_OUTER_CALL);result.replace_range(a..b,&entry);
 result.push_str(SUPPORT);result.push_str(&capture);result.push('\n');result.push_str(&narrow);result.push('\n');
 let m=naga::front::wgsl::parse_str(&result).map_err(|e|e.emit_to_string(&result))?;
 let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("outer primary validation: {e:?}"))?;
 // A prefix can be repeated or its suffix skipped only if query helpers have
 // no persistent side effects. Scoped visibility and our snapshots are explicit.
 for(name,allowed)in [
  ("retainedOriginalNearest",&[][..]),("retainedOuterNarrow",&[][..]),
  ("retainedOuterCapture",&["retainedOuterPanels","retainedOuterControls"][..]),
  ("gemNearest",&["retainedPrimary"][..]),
  ("retainedOuterBegin",&["retainedPrimary","retainedOuterAvailable","retainedOuterOrigin","retainedOuterDirection","retainedOuterGroup","retainedOuterPanels","retainedOuterControls","retainedOuterFinal"][..])
 ]{
  let(h,_)=m.functions.iter().find(|(_,f)|f.name.as_deref()==Some(name)).ok_or("missing primary function")?;
  for(g,v)in m.global_variables.iter(){if info[h][g].contains(naga::valid::GlobalUse::WRITE)&&!allowed.contains(&v.name.as_deref().unwrap_or("")){return Err(format!("outer primary unsafe write: {name} -> {:?}",v.name));}}
 }
 Ok(result)
}

#[cfg(test)]mod tests{
 use super::*;
 fn base()->String{let plain=super::super::super::super::backdrop::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();super::super::source(&plain,true,false,true).unwrap()}
 fn function<'a>(s:&'a str,n:&str)->&'a str{let(a,b)=span(s,n).unwrap();&s[a..b]}
 fn parsed(s:&str)->naga::Module{let m=naga::front::wgsl::parse_str(s).unwrap_or_else(|e|panic!("{}",e.emit_to_string(s)));naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();m}
 #[test]fn source_validates_and_original_optical_functions_are_unchanged(){
  let old=base();let new=source(&old).unwrap();let m=parsed(&old);parsed(&new);
  for(_,f)in m.functions.iter(){let n=f.name.as_ref().unwrap();if n!="gemNearest"{assert_eq!(function(&old,n),function(&new,n),"{n}");}}
  for e in &m.entry_points{if e.name!="cs_retained"{assert_eq!(function(&old,&e.name),function(&new,&e.name),"{}",e.name);}}
  assert_eq!(function(&new,"cs_retained").replace(NEW_OUTER_CALL,OUTER_CALL),function(&old,"cs_retained"));
 }
 #[test]fn captures_and_narrow_queries_differ_only_at_exact_checkpoint_boundaries(){
  let old=base();let new=source(&old).unwrap();let reference=function(&old,"retainedOriginalNearest");
  let capture=function(&new,"retainedOuterCapture").replace("fn retainedOuterCapture(","fn retainedOriginalNearest(").replace(CAPTURE_PANELS,"").replace(CAPTURE_CONTROLS,"");assert_eq!(capture,reference);
  let narrow=function(&new,"retainedOuterNarrow").replace("fn retainedOuterNarrow(","fn retainedOriginalNearest(").replace(CHECK_PANELS,"").replace(CHECK_CONTROLS,"");assert_eq!(narrow,reference);
  // No branch is added after controls: all mixer/triangle/tape budgets and ties
  // remain the exact original suffix on failure to converge.
  let at=reference.find("    var mixer:").unwrap();assert!(function(&new,"retainedOuterNarrow").ends_with(&reference[at..]));
 }
 #[test]fn frame_identity_group_transition_and_full_bit_equality_are_required(){
  let new=source(&base()).unwrap();let wrapper=function(&new,"gemNearest");
  for guard in ["retainedOuterAvailable&&primary&&medium== -1","retainedGroup==retainedOuterGroup","bitcast<vec3<u32>>(origin)==bitcast<vec3<u32>>(retainedOuterOrigin)","bitcast<vec3<u32>>(ray)==bitcast<vec3<u32>>(retainedOuterDirection)","else{hit=retainedOriginalNearest(origin,ray,medium,farLimit);}","retainedPrimary=false;"]{assert!(wrapper.contains(guard),"{guard}");}
  let equal=function(&new,"retainedOuterHitEqual");assert!(equal.contains("a.panel==b.panel"));assert!(equal.contains("bitcast<vec4<u32>>(vec4<f32>(a.distance,a.normal))"));assert!(equal.contains("retainedOuterHitFinite(a)&&retainedOuterHitFinite(b)"));
  let capture=function(&new,"retainedOuterBegin");assert!(capture.find("retainedOuterAvailable=false;").unwrap()<capture.find("retainedOuterCapture(").unwrap());
 }
 #[test]fn changed_wrapper_and_reachable_side_effects_reject(){
  let old=base();assert!(source(&old.replace(" retainedPrimary=false;\n return hit;"," return hit;")).is_err());
  let marker="fn retainedOriginalNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {";
  assert_eq!(old.matches(marker).count(),1);assert!(source(&old.replace(marker,&format!("{marker} rubyBakeLeaves+=1;"))).is_err());
  assert!(source(&source(&old).unwrap()).is_err());
 }
 #[test]fn original_meter_ledger_and_meter_base_remain_unchanged(){
  let old=base();let new=source(&old).unwrap();
  let mut old=super::super::with_meter_ledger(&old).unwrap();let mut new=super::super::with_meter_ledger(&new).unwrap();
  super::super::append_meter_base(&mut old).unwrap();super::super::append_meter_base(&mut new).unwrap();parsed(&new);
  for n in ["retainedMeterTrace","meterLedgerCaptureTrace","cs_meter_ledger_capture","fs_retained","fs_retained_without_meters"]{assert_eq!(function(&old,n),function(&new,n),"{n}");}
  // The promoted playhead certificate uses a separate capture-only module.
  // Its instrumentation must not leak into the ordinary atlas module.
  let capture=super::super::meter_ledger_capture_source(&new,true).unwrap().unwrap();parsed(&capture);
  assert!(!new.contains("meterLedgerPlaybackCapture"));assert!(capture.contains("meterLedgerPlaybackCapture"));
 }
 #[derive(Clone,Copy,Debug,PartialEq)]struct Hit{distance:u32,normal:[u32;3],panel:i32}
 fn same(a:Hit,b:Hit)->bool{a==b&&f32::from_bits(a.distance).is_finite()&&a.normal.into_iter().all(|v|f32::from_bits(v).is_finite())}
 #[test]fn cpu_hit_comparison_rejects_normal_ties_signed_zero_and_nonfinite(){
  let h=Hit{distance:2f32.to_bits(),normal:[0,0,1f32.to_bits()],panel:9};assert!(same(h,h));
  for changed in [Hit{panel:10,..h},Hit{normal:[1,0,1f32.to_bits()],..h},Hit{normal:[(-0f32).to_bits(),0,1f32.to_bits()],..h},Hit{distance:f32::NAN.to_bits(),..h},Hit{distance:f32::INFINITY.to_bits(),..h}]{assert!(!same(h,changed));}
  let nan=Hit{distance:f32::NAN.to_bits(),..h};assert!(!same(nan,nan));
 }
}
