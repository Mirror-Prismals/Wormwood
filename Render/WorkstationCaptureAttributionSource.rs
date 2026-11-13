//! Changed-output DIAGNOSTICS only. Use a separate synchronous cs_retained
//! pipeline, never ordinary render/response/incremental pipelines or shipping
//! defaults. Packets, invalidation, sample count and capture schedule stay owned
//! by the unchanged host. Timing deltas are not isolated instruction counts.
pub(crate) const MODES:[&str;7]=[
 "omit_io_geometry","omit_mixer_housing_slider","omit_meter_geometry",
 "neutralize_io_engraving_light","neutralize_all_engraving","flat_dynamic_optics","flat_all_after_primary"
];
const MARKER:&str="// CAPTURE_ATTRIBUTION_CHANGED_OUTPUT_V1";
const IO_INSTANCE:&str="(instance>=29&&instance<61)||(instance>=65&&instance<125)";
const IO_MEDIUM:&str="(medium>=7428&&medium<15620)||(medium>=16644&&medium<32004)";
const ENGRAVING:[&str;7]=["gemEngraving","gemControlEngraving","gemBarLoopEngraving","gemMixerEngraving","gemTimelineEngraving","gemPluginNavigationEngraving","gemTrackIoEngraving"];
fn range(s:&str,name:&str)->Result<(usize,usize,usize),String>{let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("attribution requires one {name}"));}let start=s.find(&marker).unwrap();let open=start+s[start..].find('{').ok_or("missing function body")?;let mut depth=0usize;for(i,b)in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok((start,open,i+1));}}}Err("unclosed attribution target".into())}
fn function<'a>(s:&'a str,name:&str)->Result<&'a str,String>{let(a,_,b)=range(s,name)?;Ok(&s[a..b])}
fn require(s:&str,marker:&str)->Result<(),String>{if s.matches(marker).count()!=1{return Err(format!("attribution source guard changed: {marker}"));}Ok(())}
fn prepend(s:&mut String,name:&str,guard:&str,code:&str)->Result<(),String>{let(_,open,end)=range(s,name)?;require(&s[open..end],guard)?;s.insert_str(open+1,code);Ok(())}
fn replace_local(s:&mut String,name:&str,old:&str,new:&str)->Result<(),String>{let(a,_,b)=range(s,name)?;require(&s[a..b],old)?;let next=s[a..b].replacen(old,new,1);s.replace_range(a..b,&next);Ok(())}
fn validate(s:&str)->Result<(),String>{let m=naga::front::wgsl::parse_str(s).map_err(|e|e.emit_to_string(s))?;naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("attribution WGSL: {e:?}"))?;Ok(())}
/// "off" is an exact identity, including no parsing or marker insertion.
pub(crate) fn transform(input:&str,mode:&str)->Result<String,String>{
 if mode=="off"{return Ok(input.to_owned());}
 if !MODES.contains(&mode){return Err(format!("unknown capture attribution mode {mode}"));}
 if input.contains(MARKER)||input.contains("captureAttributionIoLight"){return Err("capture attribution already installed/reserved name occupied".into());}
 function(input,"cs_retained")?;validate(input)?;
 let mut s=input.to_owned();
 match mode{
 "omit_io_geometry"=>{
  // Document lane lights29..60 and right-page controls65..124 only.
  let poses=function(&s,"gemControlPoseRow")?;
  for guard in ["if(instance>=125)","if(instance>=65)","if(instance>=29)"]{require(poses,guard)?;}
  prepend(&mut s,"gemControlExit","var instance:i32=(medium-4)/256;",&format!("\n if({IO_MEDIUM}){{return GemHit(farLimit,vec3<f32>(0.0),-1);}}\n"))?;
  prepend(&mut s,"gemControlHit","if(medium>=4){return gemControlExit(origin,ray,medium,farLimit);}",&format!("\n if({IO_MEDIUM}){{return GemHit(farLimit,vec3<f32>(0.0),-1);}}\n"))?;
  let old="var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);";
  replace_local(&mut s,"gemControlHit",old,&format!("if({IO_INSTANCE}){{continue;}}\n        {old}"))?;
 },
 "omit_mixer_housing_slider"|"omit_meter_geometry"=>{
  let condition=if mode=="omit_meter_geometry"{"instance%3==2"}else{"instance%3!=2"};
  prepend(&mut s,"gemMixerActive","var at:i32=gemMixerRecord(instance);",&format!("\n if({condition}){{return false;}}\n"))?;
  // Covers direct calls as well as gemMixerHit's active-instance loop.
  prepend(&mut s,"gemMixerTraceInstance","var hit:GemHit=GemHit(farLimit,vec3<f32>(0.0),-1);",&format!("\n if({condition}){{return GemHit(farLimit,vec3<f32>(0.0),-1);}}\n"))?;
 },
 "neutralize_io_engraving_light"=>{
  let target=function(&s,"gemTrackIoEngraving")?;
  for guard in ["if(instance<65||instance>=129||abs(ray.z)<0.00001)","var alpha:f32=1.0-exp(","transmission*=1.0-alpha;"]{require(target,guard)?;}
  replace_local(&mut s,"gemTrackIoEngraving","gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w)","captureAttributionIoLight(pose.xyz+point*pose.w,medium,pose.w)")?;
  s.push_str("\nfn captureAttributionIoLight(point:vec3<f32>,medium:i32,scale:f32)->vec3<f32>{\n if(medium>=16644&&medium<32004){return vec3<f32>(1.0);}\n return gemControlEngravingLight(point,medium,scale);\n}\n");
 },
 "neutralize_all_engraving"=>{
  for name in ENGRAVING{let(_,open,end)=range(&s,name)?;if !s[..open].ends_with("->vec4<f32> "){return Err(format!("attribution engraving return signature changed: {name}"));}s.replace_range(open+1..end-1,"\n return vec4<f32>(0.0,0.0,0.0,1.0);\n");}
 },
 "flat_all_after_primary"=>{
  // Cost diagnostic only: preserve cs_retained's geometric coverage/depth query,
  // and rubyFastTrace's original second nearest query and miss/environment path.
  // Replace all positive-hit optical/shading work after that query with a constant.
  // Unusual nonpositive/NaN hit distances keep the original behavior.
  let marker="let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);";
  replace_local(&mut s,"rubyFastTrace",marker,&format!("if(hit.panel>=0&&hit.distance>0.0){{return vec4<f32>(0.25,0.25,0.25,0.0);}}\n    {marker}"))?;
 },
 "flat_dynamic_optics"=>{
  // Lane monitors have a primary opaque shortcut; right-page controls, mixer
  // and bars ordinarily fall back. Reject the extended experimental shortcut.
  require(function(&s,"rubyFastTrace")?,"if((hit.panel>=0&&hit.panel<16644)||(hit.panel>=700000&&hit.panel<704096)){")?;
  // Lane lights29..60 can already take the primary opaque shortcut: flatten
  // that path too, after its unchanged exact nearest query and before shading.
  let fast_hit="let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);";
  let fast_flat="\n    if(hit.distance>0.0&&((hit.panel>=7428&&hit.panel<15620)||(hit.panel>=16644&&hit.panel<32004)||(hit.panel>=800000&&hit.panel<853760)||(hit.panel>=720000&&hit.panel<720003))){return vec4<f32>(gemRow(gemPanelRow(hit.panel)+4).xyz,0.0);}";
  replace_local(&mut s,"rubyFastTrace",fast_hit,&format!("{fast_flat}\n    {fast_hit}"))?;
  let trace=function(&s,"rubyTraceFallback")?;
  for guard in ["var pending:array<GemRay,16>;", "var hit:GemHit=gemNearest(current.origin,current.direction,current.medium,environment.w);"]{require(trace,guard)?;}
  let old="        if(!finished&&!membrane){";
  let flat="        if(!finished&&!membrane&&((hit.panel>=7428&&hit.panel<15620)||(hit.panel>=16644&&hit.panel<32004)||(hit.panel>=800000&&hit.panel<853760)||(hit.panel>=720000&&hit.panel<720003))){\n            result+=current.weight*gemRow(gemPanelRow(hit.panel)+4).xyz;finished=true;\n        }\n";
  replace_local(&mut s,"rubyTraceFallback",old,&format!("{flat}{old}"))?;
 },_=>unreachable!()
 }
 s.push_str(&format!("\n{MARKER}: {mode}; image intentionally differs.\n"));validate(&s)?;Ok(s)
}

#[cfg(test)]mod tests{
 use super::*;
 const FIXTURE:&str=r#"
struct GemHit{distance:f32,normal:vec3<f32>,panel:i32};
struct GemRay{origin:vec3<f32>,weight:vec3<f32>};
fn gemRow(at:i32)->vec4<f32>{return vec4<f32>(f32(at));}
fn gemPanelRow(panel:i32)->i32{return panel;}
fn gemControlPoseRow(instance:i32)->i32{if(instance>=125){return 44596;}if(instance>=65){return 44292;}if(instance>=29){return 247;}return 192;}
fn gemControlExit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit{var instance:i32=(medium-4)/256;return GemHit(farLimit,ray,instance);}
fn gemControlHit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32,anyHit:bool)->GemHit{
 if(medium>=4){return gemControlExit(origin,ray,medium,farLimit);}
 for(var instance:i32=0;instance<129;instance++){var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);if(monitor&&anyHit){return GemHit(farLimit,ray,instance);}}
 return GemHit(farLimit,ray,-1);}
fn gemMixerRecord(instance:i32)->i32{return instance;}
fn gemMixerActive(instance:i32)->bool{var at:i32=gemMixerRecord(instance);return at>=0;}
fn gemMixerTraceInstance(origin:vec3<f32>,ray:vec3<f32>,instance:i32,medium:i32,farLimit:f32,anyHit:bool)->GemHit{var hit:GemHit=GemHit(farLimit,vec3<f32>(0.0),-1);return hit;}
fn gemControlEngravingLight(point:vec3<f32>,medium:i32,scale:f32)->vec3<f32>{return point*scale;}
fn gemTrackIoEngraving(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->vec4<f32> {
 var instance:i32=(medium-4)/256;if(instance<65||instance>=129||abs(ray.z)<0.00001){return vec4<f32>(0.0,0.0,0.0,1.0);}
 var pose:vec4<f32>=gemRow(instance);var point:vec3<f32>=origin;var transmission:f32=1.0;
 var alpha:f32=1.0-exp(-ray.z);var result:vec3<f32>=transmission*alpha*gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w);
 transmission*=1.0-alpha;return vec4<f32>(result,transmission);}
fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit{return gemControlHit(origin,ray,medium,farLimit,false);}
fn rubyResponseAddress(origin:vec3<f32>,direction:vec3<f32>,panel:i32,random:vec2<f32>)->i32{return -1;}
fn rubyFastTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec4<f32>{let environment=vec4<f32>(1e20);let hit=gemNearest(origin,direction,-1,environment.w);if(hit.panel<0||hit.distance<=0.0){return vec4<f32>(0.0);}let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);if((hit.panel>=0&&hit.panel<16644)||(hit.panel>=700000&&hit.panel<704096)){return vec4<f32>(1.0);}return vec4<f32>(0.0,0.0,0.0,-1.0);}
fn rubyTraceFallback(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32>{
 var pending:array<GemRay,16>;var current:GemRay=GemRay(origin,direction);var environment:vec4<f32>=vec4<f32>(1e20);var result:vec3<f32>=vec3<f32>(0.0);
 var hit:GemHit=gemNearest(current.origin,current.direction,current.medium,environment.w);
 var finished:bool=false;var membrane:bool=false;
        if(!finished&&!membrane){result+=current.weight;}
 return result;}
@compute @workgroup_size(1)fn cs_retained(){let a=rubyTraceFallback(vec3<f32>(0.0),vec3<f32>(1.0),vec2<f32>(0.0));}
"#;
 fn fixture()->String{let mut s=FIXTURE.replace("struct GemRay{origin:vec3<f32>,weight:vec3<f32>}","struct GemRay{origin:vec3<f32>,direction:vec3<f32>,medium:i32,weight:vec3<f32>}").replace("GemRay(origin,direction)","GemRay(origin,direction,-1,vec3<f32>(1.0))");for name in ENGRAVING{if name!="gemTrackIoEngraving"{s.push_str(&format!("\nfn {name}(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->vec4<f32> {{return vec4<f32>(origin,farLimit);}}\n"));}}s}
 #[test]fn off_is_exact_and_unknown_or_duplicate_modes_fail(){let bytes="not WGSL\n  \t";assert_eq!(transform(bytes,"off").unwrap(),bytes);let s=fixture();assert!(transform(&s,"unknown").is_err());let a=transform(&s,"omit_io_geometry").unwrap();assert!(transform(&a,"omit_io_geometry").is_err());}
 #[test]fn all_variants_naga_validate_and_do_not_modify_capture_entry(){let s=fixture();for mode in MODES{let out=transform(&s,mode).unwrap();assert_eq!(function(&out,"cs_retained").unwrap(),function(&s,"cs_retained").unwrap());assert!(out.ends_with(&format!("{MARKER}: {mode}; image intentionally differs.\n")));}}
 #[test]fn light_only_changes_one_call_preserving_alpha_and_other_engraving(){let s=fixture();let out=transform(&s,"neutralize_io_engraving_light").unwrap();let expected=function(&s,"gemTrackIoEngraving").unwrap().replace("gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w)","captureAttributionIoLight(pose.xyz+point*pose.w,medium,pose.w)");assert_eq!(function(&out,"gemTrackIoEngraving").unwrap(),expected);assert_eq!(function(&out,"gemControlEngravingLight").unwrap(),function(&s,"gemControlEngravingLight").unwrap());assert!(out.contains("if(medium>=16644&&medium<32004){return vec3<f32>(1.0);}"));}
 #[test]fn flat_mode_covers_primary_opaque_monitor_and_fallback_optics(){let s=fixture();let out=transform(&s,"flat_dynamic_optics").unwrap();for name in ["rubyFastTrace","rubyTraceFallback"]{let f=function(&out,name).unwrap();assert!(f.contains("hit.panel>=7428&&hit.panel<15620"));assert!(f.contains("hit.panel>=16644&&hit.panel<32004"));assert!(f.contains("hit.panel>=800000&&hit.panel<853760"));assert!(f.contains("hit.panel>=720000&&hit.panel<720003"));assert!(f.contains("gemRow(gemPanelRow(hit.panel)+4).xyz"));}}
 #[test]fn family_boundaries_preserve_legacy_monitors_navigation_and_keyboard(){for i in 0..128{let expected=(29..61).contains(&i)||(65..125).contains(&i);for mat in [0,255]{let m=4+i*256+mat;let actual=(7428..15620).contains(&m)||(16644..32004).contains(&m);assert_eq!(expected,actual);}}for i in 0..210{assert_ne!(i%3==2,i%3!=2);}}
 #[test]fn target_guard_changes_fail_instead_of_silent_partial_ablation(){let s=fixture();assert!(transform(&s.replace("if(instance>=125)","if(instance>=124)"),"omit_io_geometry").is_err());assert!(transform(&s.replace("transmission*=1.0-alpha;","transmission=1.0;"),"neutralize_io_engraving_light").is_err());assert!(transform(&s.replace("hit.panel<16644","hit.panel<32772"),"flat_dynamic_optics").is_err());}
 #[test]fn flat_all_keeps_both_primary_query_and_miss_path_while_only_fast_trace_changes(){
  let s=fixture();let out=transform(&s,"flat_all_after_primary").unwrap();
  let old=function(&s,"rubyFastTrace").unwrap();let new=function(&out,"rubyFastTrace").unwrap();
  let marker="let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);";
  assert_eq!(old.split(marker).next().unwrap(),new.split("if(hit.panel>=0&&hit.distance>0.0)").next().unwrap());
  assert!(new.contains("return vec4<f32>(0.25,0.25,0.25,0.0)"));
  for name in ["gemNearest","gemControlExit","gemControlHit","rubyTraceFallback","cs_retained"]{assert_eq!(function(&s,name).unwrap(),function(&out,name).unwrap());}
 }

}
