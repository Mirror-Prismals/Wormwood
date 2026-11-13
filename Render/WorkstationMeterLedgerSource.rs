//! Opt-in GPU-only ordered meter ledger. Exact visible aperture tests and
//! canonical overflow/missing fallback remain in fs_retained. Cache ownership,
//! complete dependency keys and completion publication belong to the renderer.
#[path="WorkstationMeterLedgerPlayback.rs"] mod playback;
const MARKER:&str="// METER_LEDGER_PRODUCTION_BEGIN";
const CAP:u32=16;
const TERM_BYTES:u32=48;
const SLOT_BYTES:u32=16+CAP*TERM_BYTES;

fn replace_once(s:&mut String,a:&str,b:&str)->Result<(),String>{
 if s.matches(a).count()!=1{return Err(format!("meter ledger marker changed: {a}"));}
 *s=s.replacen(a,b,1);Ok(())
}
fn block_end(s:&str,open:usize)->Result<usize,String>{
 let mut depth=0;
 for(i,c)in s[open..].char_indices(){if c=='{'{depth+=1;}else if c=='}'{depth-=1;if depth==0{return Ok(open+i+1);}}}
 Err("unclosed meter ledger source block".into())
}
fn function<'a>(s:&'a str,name:&str)->Result<&'a str,String>{
 let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("expected one {name}"));}
 let start=s.find(&marker).unwrap();let open=start+s[start..].find('{').ok_or("missing function body")?;
 Ok(&s[start..block_end(s,open)?])
}

fn forced_color(s:&str)->Result<String,String>{
 let mut color=function(s,"gemMixerMeterColor")?.to_string();
 replace_once(&mut color,"fn gemMixerMeterColor(point:vec3<f32>,medium:i32)->vec4<f32>",
     "fn meterLedgerForcedColor(point:vec3<f32>,medium:i32,strength:f32)->vec4<f32>")?;
 let selector="var levels:vec4<f32>=gemRow(gemMixerRecord(instance)+3);var fill:f32=levels.z;if(channel==1){fill=levels.w;}fill=clamp(fill,0.0,1.0);var strength:f32=0.25;if(y<fill){strength=1.0;}";
 replace_once(&mut color,selector,
     "meterLedgerSelector=u32(gemMixerRecord(instance)+3)*4u+u32(2+channel);meterLedgerThreshold=y;")?;
 Ok(color)
}

fn opaque_state(trace:&str)->Result<String,String>{
 let marker="            if(material.w<0.0001){";
 if trace.matches(marker).count()!=1{return Err("canonical opaque branch changed".into());}
 let open=trace.find(marker).unwrap()+marker.len()-1;
 let mut body=trace[open+1..block_end(trace,open)?-1].to_string();
 replace_once(&mut body,"var meterColor:vec4<f32>=gemMixerMeterColor(point,hit.panel);",
     "var meterColor:vec4<f32>=meterLedgerForcedColor(point,hit.panel,strength);")?;
 replace_once(&mut body,"result+=current.weight*(opaque+emission);","return opaque+emission;")?;
 replace_once(&mut body,"finished=true;","")?;
 body=body.replace("-current.direction","-direction").replace("hit.panel","panel");
 if body.contains("current.")||body.contains("result+=")||body.contains("finished"){
  return Err("unexpected opaque state dependencies".into());
 }
 Ok(format!("fn meterLedgerOpaqueState(point:vec3<f32>,direction:vec3<f32>,n:vec3<f32>,panel:i32,strength:f32)->vec3<f32>{{\nlet base=gemPanelRow(panel);let material=gemRow(base);{body}\n}}"))
}

fn instrument(trace:&str)->Result<String,String>{
 let mut replacements=Vec::new();let mut cursor=0;
 while let Some(relative)=trace[cursor..].find("result+="){
  let start=cursor+relative;let end=start+trace[start..].find(';').ok_or("missing accumulator semicolon")?+1;
  let expression=&trace[start+8..end-1];
  let (weight,radiance)=expression.split_once('*').ok_or("unknown meter accumulation")?;
  if !matches!(weight,"current.weight"|"reflectWeight"|"transmitWeight"){
   return Err(format!("unknown meter accumulation weight: {weight}"));
  }
  let record=if expression=="current.weight*(opaque+emission)"{
   "meterLedgerRecordOpaque(current.weight,opaque+emission,point,current.direction,n,hit.panel,meterColor.w>0.5);".to_string()
  }else{format!("meterLedgerRecordConstant({weight},{radiance});")};
  // Keep the original accumulator expression and all ray scheduling untouched.
  // Helpers used as a radiance operand may be evaluated twice during capture;
  // only capture pays that cost. No original accumulator statement is replaced.
  replacements.push((start,record));cursor=end;
 }
 if replacements.len()!=18{return Err(format!("canonical result addition sites changed: {}",replacements.len()));}
 let mut result=trace.to_string();
 for(start,record)in replacements.into_iter().rev(){result.insert_str(start,&record);}
 replace_once(&mut result,"fn retainedMeterTrace(","fn meterLedgerCaptureTrace(")?;
 Ok(result)
}

#[cfg(test)]mod tests{
 use super::*;use std::collections::BTreeSet;
 fn source(s:&str)->Result<String,String>{super::source_with_playhead(s,false)}
 const OLD_CALL:&str="best=vec4<f32>(retainedMeterTrace(origin,direction,random),hit.distance);";
 const NEW_CALL:&str="let ledger=meterLedgerLookup(p,sample);\n    if(ledger.w>0.5){best=vec4<f32>(ledger.xyz,hit.distance);}\n    else{best=vec4<f32>(retainedMeterTrace(origin,direction,random),hit.distance);}";
 pub(super) fn base()->String{
  let plain=super::super::super::super::backdrop::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
  super::super::source(&plain,true,false,true).unwrap()
 }
 fn validate(s:&str)->(naga::Module,naga::valid::ModuleInfo){
  let m=naga::front::wgsl::parse_str(s).unwrap_or_else(|e|panic!("{}",e.emit_to_string(s)));
  let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();(m,info)
 }
 fn bindings(m:&naga::Module,info:&naga::valid::ModuleInfo,name:&str)->BTreeSet<u32>{
  let i=m.entry_points.iter().position(|e|e.name==name).unwrap();let usage=info.get_entry_point(i);
  m.global_variables.iter().filter_map(|(h,v)|if usage[h].is_empty(){None}else{v.binding.as_ref().map(|b|{assert_eq!(b.group,0);b.binding})}).collect()
 }
 #[test]fn production_entries_validate_and_have_only_required_bindings(){
  let s=source(&base()).unwrap();let(m,info)=validate(&s);
  super::super::diagnostic_call_graph(&s).unwrap();
  assert_eq!(bindings(&m,&info,"cs_meter_ledger_capture"),BTreeSet::from([1,6,9,10]));
  assert_eq!(bindings(&m,&info,"fs_retained"),BTreeSet::from([1,5,6,9,13]));
  assert!(!m.entry_points.iter().any(|e|e.name.contains("probe")));
  assert_eq!(m.entry_points.iter().find(|e|e.name=="cs_meter_ledger_capture").unwrap().workgroup_size,[64,1,1]);
 }
 #[test]fn original_fragment_changes_only_accepted_meter_radiance_call(){
  let old=base();let new=source(&old).unwrap();
  assert_eq!(function(&new,"fs_retained").unwrap().replace(NEW_CALL,OLD_CALL),function(&old,"fs_retained").unwrap());
  for n in ["retainedMeterTrace","gemMixerMeterColor","retainedRay","gemMixerTraceInstance","gemTimelinePlayhead","gemDisplay","cs_retained"]{assert_eq!(function(&old,n).unwrap(),function(&new,n).unwrap(),"{n}");}
 }
 #[test]fn meter_base_append_keeps_same_source_and_no_ledger_bindings(){
  let old=base();let mut ordinary=old.clone();super::super::append_meter_base(&mut ordinary).unwrap();
  let mut ledger=source(&old).unwrap();super::super::append_meter_base(&mut ledger).unwrap();
  assert_eq!(function(&ordinary,"fs_retained_without_meters").unwrap(),function(&ledger,"fs_retained_without_meters").unwrap());
  let(m,info)=validate(&ledger);assert_eq!(bindings(&m,&info,"fs_retained_without_meters"),BTreeSet::from([1,5,6]));
 }
 #[test]fn all_eighteen_accumulations_keep_exact_order_and_operands(){
  let old=base();let trace=function(&old,"retainedMeterTrace").unwrap();let new=instrument(trace).unwrap();
  let expressions=|s:&str|s.match_indices("result+=").map(|(i,_)|s[i..i+s[i..].find(';').unwrap()+1].to_owned()).collect::<Vec<_>>();
  assert_eq!(expressions(trace).len(),18);assert_eq!(expressions(trace),expressions(&new));
  assert_eq!(new.matches("meterLedgerRecordConstant(").count(),17);assert_eq!(new.matches("meterLedgerRecordOpaque(").count(),1);
  assert!(new.contains("event<64"));assert!(new.contains("array<GemRay,16>"));
 }
 #[test]fn exact_threshold_full_opaque_alternatives_and_slot_abi_preserved(){
  let old=base();let forced=forced_color(&old).unwrap();
  assert!(forced.contains("var y:f32=clamp((local.y+0.078)/0.156,0.0,1.0)"));assert!(forced.contains("meterLedgerThreshold=y"));
  assert!(forced.contains("mix(a,b,blend)*strength"));assert!(!forced.contains("levels.z"));assert!(!forced.contains("levels.w"));
  let opaque=opaque_state(function(&old,"retainedMeterTrace").unwrap()).unwrap();
  for text in ["emission=surfaceColor*0.85","gemOpaque(point,-direction,n,surfaceColor,material.z,false)","return opaque+emission"]{assert!(opaque.contains(text));}
  assert_eq!((TERM_BYTES,SLOT_BYTES),(48,784));let(m,_)=validate(&source(&old).unwrap());
  for(name,size)in [("MeterLedgerTerm",48),("MeterLedgerSlot",784),("MeterLedgerJob",16)]{
   let t=m.types.iter().find(|(_,t)|t.name.as_deref()==Some(name)).unwrap().1;
   let naga::TypeInner::Struct{span,..}=t.inner else{panic!("not struct")};assert_eq!(span,size);
  }
 }
 #[test]fn capture_has_no_reference_probe_or_metadata_mode_and_lookup_fails_closed(){
  let code=include_str!("WorkstationMeterLedger.wgsl");let capture=function(code,"cs_meter_ledger_capture").unwrap();
  for forbidden in ["retainedMeterTrace(","meterLedgerEvaluate(","meterLedgerProbes","retainedMeta.rows[0]"]{assert!(!capture.contains(forbidden));}
  assert!(capture.contains("vec4<u32>(state,meterLedgerCount,address,job.sample)"));
  for guard in ["if(base==0xffffffffu)","if(address>=arrayLength(&meterLedgerMap.pixels))","if(index>=arrayLength(&meterLedgerSlots.slots))","if(header.z!=address||header.w!=u32(sample))","if(header.x!=1u||header.y>16u)","if(index>=16u){meterLedgerOverflow=true;return;}"]{assert!(code.contains(guard),"{guard}");}
 }
 #[test]fn capture_module_is_separate_and_default_ordinary_source_stays_exact(){
  let base=base();let ordinary=source(&base).unwrap();let saved=ordinary.clone();
  assert!(super::capture_source(&ordinary,false).unwrap().is_none());
  let capture=super::capture_source(&ordinary,true).unwrap().unwrap();
  assert_eq!(ordinary,saved);
  assert_eq!(super::super::with_meter_ledger(&base).unwrap(),ordinary);
  assert!(!ordinary.contains("// METER_LEDGER_PLAYBACK_CERTIFICATE"));
  assert!(capture.contains("// METER_LEDGER_PLAYBACK_CERTIFICATE"));
  let(m,info)=validate(&capture);assert_eq!(bindings(&m,&info,"cs_meter_ledger_capture"),BTreeSet::from([1,6,9,10]));
  let(m,info)=validate(&ordinary);assert_eq!(bindings(&m,&info,"fs_retained"),BTreeSet::from([1,5,6,9,13]));
  assert!(super::capture_source(&capture,true).is_err());
  assert!(super::capture_source(&base,true).is_err());
 }
 #[test]fn capture_module_accepts_final_camera_meter_partition_source(){
  let mut ordinary=source(&base()).unwrap();super::super::append_meter_base(&mut ordinary).unwrap();
  ordinary.push_str(include_str!("WorkstationCameraColor.wgsl"));
  let capture=super::capture_source(&ordinary,true).unwrap().unwrap();let(m,info)=validate(&capture);
  assert_eq!(bindings(&m,&info,"cs_meter_ledger_capture"),BTreeSet::from([1,6,9,10]));
  assert!(!ordinary.contains("meterLedgerPlaybackCapture"));
 }
 #[test]fn malformed_duplicate_or_conflicting_source_rejected(){
  let old=base();let built=source(&old).unwrap();assert!(source(&built).is_err());
  let trace=function(&old,"retainedMeterTrace").unwrap();let changed=trace.replacen("result+=","result=result+",1);
  assert!(source(&old.replacen(trace,&changed,1)).is_err());assert!(source(&old.replace(OLD_CALL,"best=vec4<f32>(0.0);")).is_err());
  assert!(source(&(old+"\n@group(0) @binding(13) var<storage,read> conflict:array<u32>;" )).is_err());
 }
}

// Derive a separate capture-only module from the final ordinary module. The
// caller must never use this result for retained capture or fragment pipelines.
pub(super) fn capture_source(s:&str,playhead:bool)->Result<Option<String>,String>{
 if !s.contains(MARKER)||s.contains("// METER_LEDGER_PLAYBACK_CERTIFICATE"){
  return Err("meter ledger capture requires one uninstrumented production module".into());
 }
 if playhead{Ok(Some(playback::source(s)?))}else{Ok(None)}
}
pub(super) fn source(s:&str)->Result<String,String>{
 source_with_playhead(s,std::env::var("WORMWOOD_WORKSTATION_METER_LEDGER_PLAYHEAD").as_deref()==Ok("1"))
}
pub(super) fn source_with_playhead(s:&str,playhead:bool)->Result<String,String>{
 if s.contains(MARKER){return Err("meter ledger already appended".into());}
 if s.contains("@binding(9)")||s.contains("@binding(10)")||s.contains("@binding(13)"){
  return Err("meter ledger production binding collision".into());
 }
 let trace=function(s,"retainedMeterTrace")?;
 // Terminal substitution must be the actual current retained-meter path.
 if !trace.contains("retainedMeterTerminal(")||trace.contains("gemTerminal("){
  return Err("unexpected retained meter terminal policy".into());
 }
 let mut result=s.to_string();
 replace_once(&mut result,
  "best=vec4<f32>(retainedMeterTrace(origin,direction,random),hit.distance);",
  "let ledger=meterLedgerLookup(p,sample);\n    if(ledger.w>0.5){best=vec4<f32>(ledger.xyz,hit.distance);}\n    else{best=vec4<f32>(retainedMeterTrace(origin,direction,random),hit.distance);}")?;
 result.push_str("\n");result.push_str(MARKER);result.push_str("\n");
 result.push_str(include_str!("WorkstationMeterLedger.wgsl"));
 result.push_str(&forced_color(s)?);result.push('\n');
 result.push_str(&opaque_state(trace)?);result.push('\n');
 result.push_str(&instrument(trace)?);result.push('\n');
 if playhead{result=playback::source(&result)?;}
 Ok(result)
}

