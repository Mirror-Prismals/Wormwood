#[path="WorkstationPrimaryGroupGatesSource.rs"]
mod primary_group_gates;
#[path="WorkstationPrimaryWitnessSource.rs"]
mod primary_witness;
#[path="WorkstationZeroAlphaLightSource.rs"]
mod zero_alpha_light;
#[path="WorkstationDensityZeroGuard.rs"]
mod density_zero_guard;
#[path="WorkstationOuterPrimary.rs"]
mod outer_primary;
// The original optical source remains authoritative; only primary visibility
// is partitioned while baking. Secondary rays keep the original complete scene.
#[path="WorkstationBarPrimary.rs"]
mod bar_primary;
#[path="WorkstationLinearStudioTrace.rs"]
mod linear_studio_trace;
#[path="WorkstationMeterLedgerSource.rs"]
mod meter_ledger;
// Every ordinary bake/render entry uses the validated, uninstrumented helpers.
pub(super) fn with_meter_ledger(source:&str)->Result<String,String>{meter_ledger::source_with_playhead(source,false)}
pub(super) fn meter_ledger_capture_source(source:&str,playhead:bool)->Result<Option<String>,String>{meter_ledger::capture_source(source,playhead)}
pub(super) fn offline_faders_enabled()->bool{
 std::env::var("WORMWOOD_OFFLINE_FADERS").as_deref()!=Ok("0")
}
fn offline_faders(s:&mut String)->Result<(),String>{
 let skip="if(f32(instance/3)>=gemRow(43152).x){break;}if(!gemMixerActive(instance)){continue;}";
 let overlay="  // The authored LEDs sit behind a refractive protective window.";
 if s.matches(skip).count()!=1||s.matches(overlay).count()!=1{return Err("fader composition requires the mixer and retained surface".into());}
 *s=s.replace(skip,&format!("{skip}if(retainedPrimary&&instance%3==1){{continue;}}"))
     .replace(overlay,&format!("  best=retainedFaderCap(origin,best);\n{overlay}"));
 s.push_str(include_str!("WorkstationFaderCap.wgsl"));Ok(())
}
pub(super) fn panel_detail(source:&str,original:&str)->Result<String,String>{
 let playback="let v=panelAtlasSample(group,source,rect,xy,sample);";
 let capture="else if(h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),h.distance);}";
 let plain="if(group==0||h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),select(h.distance,1e19,group==0));}";
 if source.matches(playback).count()!=1 || (!source.contains(capture)&&!source.contains(plain)) {return Err("panel detail requires packed retained panel layers".into());}
 let mut s=source.replace(playback,"let v=panelLighting(group,p,sample,panelAtlasSample(group,source,rect,xy,sample));");
 // Group 5 captures the isolated transport controls. Ruler tiles in the
 // same atlas have their own capture branch and do not enter this block.
 let shade="var rgb:vec3<f32>;if((group>=1&&group<=4&&h.panel==group-1)||(retainedTransportButtons&&group==5&&h.panel>=4&&h.panel<1028)){rgb=panelDetailTrace(origin,direction,random);}else{rgb=gemTrace(origin,direction,random);}";
 s=s.replace(capture,&format!("else if(h.panel>=0){{{shade}value=vec4<f32>(rgb,h.distance);}}"))
    .replace(plain,&format!("if(group==0||h.panel>=0){{{shade}value=vec4<f32>(rgb,select(h.distance,1e19,group==0));}}"));
 s.push_str(&crate::workstation::ruby_response_shader::canonical_trace(original)?.replacen("fn gemTrace(","fn panelDetailTrace(",1));
 s.push_str(include_str!("WorkstationPanelDetail.wgsl"));
 Ok(s)
}
pub(super) fn source(original:&str,merge_static:bool,depth_first:bool,compact:bool)->Result<String,String> {
 let density_original=density_zero_guard::selected(original)?;
 let zero_alpha_original=zero_alpha_light::transform(&density_original,std::env::var("WORMWOOD_WORKSTATION_ZERO_ALPHA_LIGHT").as_deref()==Ok("1"))?;
 let original=zero_alpha_original.as_str();
 let bar_source=if std::env::var("WORMWOOD_WORKSTATION_BAR_PRIMARY_GATE").as_deref()==Ok("1"){Some(bar_primary::source(original)?)}else{None};
 let original=bar_source.as_deref().unwrap_or(original);
 if compact&&(!merge_static||depth_first){return Err("compact atlas requires merged original-color composition".into());}
 let mut s=crate::workstation::ruby_response_shader::source(original)?;
 let linear_studio=std::env::var("WORMWOOD_WORKSTATION_LINEAR_STUDIO_TRACE").as_deref()==Ok("1");
 if linear_studio&&crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_METER_LEDGER_PLAYHEAD"){return Err("linear studio trace and playhead certificate combination is unverified".into());}
 if linear_studio{s=linear_studio_trace::source(&s)?;}
 fn once(s:&mut String,a:&str,b:&str)->Result<(),String>{if s.matches(a).count()!=1{return Err(format!("retained source marker changed: {a}"));}*s=s.replacen(a,b,1);Ok(())}
 if compact {
  // The compositor draws these three gem faces against live panel bounds.
  // Keep them visible to secondary rays in every capture.
  once(&mut s,"    var hit:GemHit=gemBarHit(origin,ray,medium,farLimit);",
   "    var hit:GemHit=GemHit(farLimit,vec3<f32>(0.0),-1);\n    if(!retainedPrimary){hit=gemBarHit(origin,ray,medium,farLimit);}")?;
 }
 if compact {
  // Bottom-page buttons share the timeline mesh query but move with panel 2.
  let count="var bottomCount:i32=i32(gemRow(255).z);";
  once(&mut s,count,&format!("{count}\n    if(retainedPrimary){{if(retainedGroup==2){{count=0;}}else{{bottomCount=0;}}}}"))?;
 }
 once(&mut s,"fn gemNearest(","fn retainedOriginalNearest(")?;
 // Only the nearest-primary function is modified; gemAny and all shadows stay original.
 let start=s.find("fn retainedOriginalNearest(").ok_or("missing nearest")?;
 let end=s[start..].find("\nfn gemAny(").ok_or("missing nearest end")?+start;
 let mut f=s[start..end].to_owned();
 once(&mut f,"        if(f32(panel)>=gemRow(0).y){break;}","        if(f32(panel)>=gemRow(0).y){break;}\n        if(retainedPrimary&&retainedGroup!=panel+1){continue;}")?;
 for (call,group) in [("gemBubbleHit",5),("userStyleHit",5),("gemAudioSphereHit",5),("gemTimelineHit",5),("gemBottomSurfaceHit",5),("gemMixerHit",5),("gemPluginDropMarker",5)] {
  // Calls are guarded, including their traversal cost, and retain original order.
  let variable=match call {"gemBubbleHit"=>"bubble","userStyleHit"=>"custom","gemAudioSphereHit"=>"sphere","gemTimelineHit"=>"timeline","gemBottomSurfaceHit"=>"bottomSurface","gemMixerHit"=>"mixer",_=>"dropMarker"};
  if call=="gemMixerHit"&&!f.contains("var mixer:GemHit="){continue;}
  let a=f.find(&format!("    var {variable}:GemHit=")).ok_or("missing group call")?;
  let b=f[a..].find('\n').ok_or("missing group line")?+a;
  let line=f[a..b].to_owned();
  let guard=if compact&&call=="gemTimelineHit" {"!retainedPrimary||retainedGroup==5||retainedGroup==2".to_owned()}
   else if compact&&call=="gemMixerHit" {"!retainedPrimary||retainedGroup==2".to_owned()}
   else if call=="gemBottomSurfaceHit" {"!retainedPrimary".to_owned()}else{format!("!retainedPrimary||retainedGroup=={group}")};
  f.replace_range(a..b,&format!("    var {variable}:GemHit=GemHit(hit.distance,vec3<f32>(0.0),-1);\n    if({guard}){{{}}}",line.trim().replacen(&format!("var {variable}:GemHit="),&format!("{variable}="),1)));
 }
 once(&mut f,"    var playhead:GemHit=gemTimelinePlayhead(origin,ray,hit.distance);","    var playhead:GemHit=GemHit(hit.distance,vec3<f32>(0.0),-1);\n    if(!retainedPrimary){playhead=gemTimelinePlayhead(origin,ray,hit.distance);}")?;
 s.replace_range(start..end,&f);
 once(&mut s,"        var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);","        if(retainedPrimary&&retainedControlGroup(instance)!=retainedGroup){continue;}\n        var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);")?;
 // Live meter optics read the current packet directly, never a cached ruby response.
 let meter_trace=crate::workstation::ruby_response_shader::canonical_trace(original)?
  .replacen("fn gemTrace(","fn retainedMeterTrace(",1)
  .replace("gemTerminal(","retainedMeterTerminal(");
 s.push_str(&meter_trace);
 s.push_str(include_str!("WorkstationPrerender.wgsl"));
 if merge_static {once(&mut s,"const retainedMergeStatic:bool=false;","const retainedMergeStatic:bool=true;")?;}
 if merge_static&&!depth_first {once(&mut s,"const retainedTransportButtons:bool=false;","const retainedTransportButtons:bool=true;")?;}
 if depth_first {
  if !merge_static {return Err("depth-first composition requires merged stationary groups".into());}
  once(&mut s," textureStore(retainedOut,vec2<i32>(id.xy),i32(id.z),value);"," textureStore(retainedOut,vec2<i32>(id.xy),i32(id.z),value);\n textureStore(retainedDepthOut,vec2<i32>(id.xy),i32(id.z),vec4<f32>(value.w,0.0,0.0,0.0));")?;
  let start=s.find("@fragment fn fs_retained(").ok_or("missing retained fragment")?;
  s.truncate(start);s.push_str(include_str!("WorkstationPrerenderDepth.wgsl"));
 }
 if compact && offline_faders_enabled(){offline_faders(&mut s)?;}
 if compact {
  // Restrict only the first intersection while capturing a ruler tile.
  // Reflections and refractions still see the normal scene.
  once(&mut s,"        var panel:i32=720000+i;", "        if(retainedPrimary&&retainedRulerFace>=0&&retainedRulerFace!=i){continue;}\n        var panel:i32=720000+i;")?;
  let signature="fn retainedOriginalNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {";
  once(&mut s,signature,&format!("{signature}\n    if(retainedPrimary&&retainedRulerFace>=0){{return gemBarHit(origin,ray,medium,farLimit);}}"))?;
  let at=s.find(" if(retainedMergeStatic&&group==0){").ok_or("missing retained capture")?;
  s.insert_str(at,include_str!("WorkstationRulerCapture.wgsl"));
  s.push_str("\nvar<private> retainedRulerFace:i32=-1;\n");
  once(&mut s,"  // COMPOSITED_RULERS",r#"  for(var face:i32=0;face<3;face++){
   let pose=gemRow(45744+face);let shape=gemRow(45747+face);
   let tile=retainedMeta.rows[61+face];let halfSize=shape.xy*pose.w;
   if(shape.w<0.5||any(halfSize<=vec2<f32>(0.0))||tile.z<=0.0){continue;}
   let uv=(origin.xy-pose.xy)/halfSize*vec2<f32>(0.5,-0.5)+0.5;
   if(any(uv<vec2<f32>(0.0))||any(uv>=vec2<f32>(1.0))){continue;}
   let xy=vec2<i32>(vec2<f32>(0.0,tile.y)+floor(uv*tile.zw)+retainedMeta.rows[25].xy);
   let v=textureLoad(retainedIn,xy,sample,0);
   if(v.w>=1e19){continue;}
   let oldHalf=retainedMeta.rows[58+face].zw;
   let localZ=(retainedMeta.rows[57].x-v.w-tile.x)/min(oldHalf.x,oldHalf.y);
   let depth=origin.z-(pose.z+localZ*pose.w);
   if(depth<best.w){best=vec4<f32>(v.xyz,depth);}
  }
"#)?;
 }
 if compact {
  once(&mut s," textureStore(retainedOut,vec2<i32>(id.xy),i32(id.z),value);"," textureStore(retainedOut,vec2<i32>(id.xy)+vec2<i32>(retainedMeta.rows[5u+u32(group)*4u].xy),sample,value);")?;
  once(&mut s,"best=textureLoad(retainedIn,vec2<i32>(p),sample,0);","best=textureLoad(retainedIn,vec2<i32>(p)+vec2<i32>(retainedMeta.rows[5u].xy),sample,0);")?;
  once(&mut s,"var v=textureLoad(retainedIn,xy,10+sample,0);","var v=textureLoad(retainedIn,xy+vec2<i32>(retainedMeta.rows[25u].xy),sample,0);")?;
  once(&mut s,"let v=textureLoad(retainedIn,xy,group*2+sample,0);","let v=textureLoad(retainedIn,xy+vec2<i32>(retainedMeta.rows[at+3u].xy),sample,0);")?;
 }
 if compact {
  // This supported stationary layer contains the timeline and its lane
  // monitors. A full-view panel covers that editor; hide old coverage live,
  // instead of forcing a synchronous optical recapture at navigation time.
  once(&mut s,"winner=select(5,0,best.w==1e19);",
   "winner=select(5,0,best.w==1e19);if(gemRow(9).w>0.0){best=vec4<f32>(0.0,0.0,0.0,1e19);winner=0;}")?;
 }
 if compact {
  once(&mut s,"let v=textureLoad(retainedIn,xy+vec2<i32>(retainedMeta.rows[at+3u].xy),sample,0);",
   "let v=panelAtlasSample(group,source,rect,xy,sample);")?;
  once(&mut s,"let source=(p+vec2<f32>(0.5)-now.xy)*old.zw/max(now.zw,vec2<f32>(0.000001))+old.xy-vec2<f32>(0.5);",
   "let source=retainedPanelSource(group,p,old,now);")?;
  s.push_str(include_str!("WorkstationPanelSample.wgsl"));
 }
 super::super::backdrop::retained(&mut s)?;
 if std::env::var("WORMWOOD_WORKSTATION_OUTER_PRIMARY_REUSE").as_deref()==Ok("1"){
  if linear_studio{return Err("outer primary and linear studio combination is unverified".into());}
  s=outer_primary::source(&s)?;
 }

 if super::bar_planes::requested(){s=super::bar_planes::transform(&s)?;}
 s=primary_group_gates::selected(&s,include_str!("WorkstationPrerender.rs"),include_str!("WorkstationIncrementalBake.rs"),include_str!("../Workstation/DecorativeControls.rs"))?;
 s=primary_witness::transform(&s,std::env::var("WORMWOOD_WORKSTATION_PRIMARY_WITNESS").as_deref()==Ok("1"))?;
 let module=naga::front::wgsl::parse_str(&s).map_err(|e|e.emit_to_string(&s))?;
 let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).map_err(|e|format!("retained validation: {e:?}"))?;
 if linear_studio{linear_studio_trace::validate_restart_safety(&s,&module,&info)?;}
 Ok(s)
}

#[cfg(test)] mod depth_first_tests {
 use super::*;
 #[test] fn retained_depth_first_shader_validates_and_keeps_baseline_source(){
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  for merged in [false,true] {let old=source(original,merged,false,false).unwrap();assert!(!old.contains("retainedDepthOut"));assert!(old.contains("let v=textureLoad(retainedIn,xy,group*2+sample,0);"));}
  let candidate=source(original,true,true,false).unwrap();assert!(candidate.contains("vec4<f32>(value.w,0.0,0.0,0.0)"));assert!(candidate.contains("best=textureLoad(retainedIn,coord,layer,0);"));
  assert!(source(original,false,true,false).is_err());
 }
 #[test] fn retained_depth_first_selection_preserves_ties_missing_layers_and_sample_values(){
  let depths=[-1.0_f32,0.0,1.0,2.0,1e19,1e20,f32::INFINITY,f32::NAN];let mut checked=0;
  for mask in 0_u32..16 {for background in depths {for choice in 0..512 {
   let mut values=[[0.0_f32;4];5];values[0]=[0.1,0.2,0.3,background];
   for group in 1..5 {values[group]=[group as f32,group as f32*0.25,group as f32*0.125,depths[(choice>>(2*(group-1)))&7]];}
   let mut old=values[0];let mut winner=if old[3]==1e19{0}else{5};
   for group in 1..5 {if mask&(1<<(group-1))!=0 {let v=values[group];if v[3]<old[3]||(v[3]==old[3]&&group<winner){old=v;winner=group;}}}
   let mut depth=background;let mut winner=if depth==1e19{0}else{5};let mut index=0;
   for group in 1..5 {if mask&(1<<(group-1))!=0 {let d=values[group][3];if d<depth||(d==depth&&group<winner){depth=d;winner=group;index=group;}}}
   assert_eq!(old.map(f32::to_bits),values[index].map(f32::to_bits));checked+=1;
  }}}
  assert_eq!(checked,65536);
 }
}

#[cfg(test)] mod compact_atlas_tests {
 use super::*;
 #[test] fn detailed_panels_trace_at_capture_and_blend_only_in_composition() {
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  let retained=source(original,true,false,true).unwrap();
  let detailed=panel_detail(&retained,original).unwrap();
  let start=detailed.find("fn cs_retained(").unwrap();
  let end=start+detailed[start..].find("// Retain the captured RGB").unwrap();
  assert!(detailed[start..end].contains("retainedTransportButtons&&group==5&&h.panel>=4&&h.panel<1028"));
  assert!(detailed[start..end].contains("rgb=panelDetailTrace(origin,direction,random)"));
  let generated=super::super::incremental::tiled_source(&with_meter_ledger(&detailed).unwrap()).unwrap();
  assert!(generated.contains("let v=panelLighting(group,p,sample,panelAtlasSample("));
 }
 #[test] fn timeline_faces_draw_from_live_geometry_in_the_retained_compositor() {
  let s=source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,true).unwrap();
  assert!(s.contains("if(!retainedPrimary){hit=gemBarHit("));
  let fragment=s.split_once("@fragment fn fs_retained(").unwrap().1;
  assert!(fragment.contains("floor(uv*tile.zw)+retainedMeta.rows[25].xy"));
  assert!(!fragment.contains("retainedRulerTrace(origin,direction,random)"));
 }
 #[test] fn compact_atlas_shader_validates_and_changes_only_storage_addresses(){let original=include_str!("../Procedures/Shaders/workstation.wgsl");let packed=source(original,true,false,true).unwrap();assert!(packed.contains("let v=panelAtlasSample(group,source,rect,xy,sample);"));assert!(packed.contains("fn retainedPanelSource("));assert!(packed.contains("let origin=retainedRay(vec2<f32>(id.xy)+rect.xy,sample);"));assert!(packed.contains("gemDisplay(total*0.5)"));assert!(source(original,false,false,true).is_err());assert!(source(original,true,true,true).is_err());}
}

// Inspect the actual generated retained shader used by the frozen diagnostic.
// This is conservative reachability, not a dynamic count of taken branches.
pub(super) fn diagnostic_call_graph(text:&str)->Result<serde_json::Value,String>{
 use naga::{Statement,Expression};use std::collections::BTreeSet;
 fn calls(block:&naga::Block,out:&mut Vec<naga::Handle<naga::Function>>){for s in block.iter(){match s{
  Statement::Call{function,..}=>out.push(*function),Statement::Block(b)=>calls(b,out),
  Statement::If{accept,reject,..}=>{calls(accept,out);calls(reject,out);},
  Statement::Switch{cases,..}=>{for c in cases{calls(&c.body,out);}},
  Statement::Loop{body,continuing,..}=>{calls(body,out);calls(continuing,out);},_=>()
 }}}
 fn globals(f:&naga::Function,m:&naga::Module,out:&mut BTreeSet<u32>){for (_,e) in f.expressions.iter(){if let Expression::GlobalVariable(h)=e{if let Some(b)=&m.global_variables[*h].binding{assert_eq!(b.group,0);out.insert(b.binding);}}}}
 let m=naga::front::wgsl::parse_str(text).map_err(|e|e.emit_to_string(text))?;let mut result=serde_json::Map::new();
 for entry in ["vs_main","fs_retained"]{
  let f=&m.entry_points.iter().find(|e|e.name==entry).ok_or("missing diagnostic entry point")?.function;
  let mut pending=vec![];calls(&f.body,&mut pending);let mut seen=BTreeSet::new();let mut names=BTreeSet::new();let mut bindings=BTreeSet::new();globals(f,&m,&mut bindings);
  while let Some(h)=pending.pop(){if seen.insert(h.index()){let f=&m.functions[h];names.insert(f.name.clone().unwrap_or_default());globals(f,&m,&mut bindings);calls(&f.body,&mut pending);}}
  if entry=="fs_retained"{
   // Visible meter apertures intentionally reach the original optical integrator.
   // The graph is conservative: it cannot establish pixel coverage or cost.
   for forbidden in ["gemTrace","rubyFastTrace","rubyTraceFallback","rubyLiveEngraving"] {
    if names.contains(forbidden){return Err(format!("Cached optical response reachable from live meter: {forbidden}"));}
   }
   let expected=if text.contains("// METER_LEDGER_PRODUCTION_BEGIN"){BTreeSet::from([1,5,6,9,13])}else{BTreeSet::from([1,5,6])};
   if bindings!=expected{return Err(format!("Frozen fragment binding set changed: {bindings:?}"));}
  }else if bindings!=BTreeSet::from([0]){return Err("Frozen vertex binding set changed".into());}
  result.insert(entry.into(),serde_json::json!({"reachable_functions":names,"bindings":bindings}));
 }
 result.insert("scope".into(),serde_json::json!("Naga IR conservative call graph of actual generated shader. Visible meter apertures reach the original optical integrator through their protective glass; the remaining workstation retains its cached appearance. This graph does not measure pixel coverage or rendering cost."));
 Ok(serde_json::Value::Object(result))
}
#[cfg(test)] mod frozen_diagnostic_tests {
 #[test] fn retained_entry_points_keep_snapshot_bindings_with_meter_optics(){
  let s=super::source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,false).unwrap();
  let graph=super::diagnostic_call_graph(&s).unwrap();
  assert!(graph["fs_retained"]["reachable_functions"].as_array().unwrap().iter().any(|v|v=="gemOpaqueSurface"));
 }
}

#[cfg(test)] mod plain_backdrop_tests {
 #[test] fn retained_backdrop_has_no_baked_floor_and_keeps_gem_tracing() {
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  let flat=super::super::super::backdrop::source(original).unwrap();
  for (merged,depth,compact) in [(false,false,false),(true,false,false),(true,true,false),(true,false,true)] {
   let s=super::source(&flat,merged,depth,compact).unwrap();
   assert!(!s.contains("value=vec4<f32>(gemTrace(origin,direction,random),1e19);"));
   assert!(s.contains("if(best.w==1e19){best=vec4<f32>(workstationBackdrop(origin,direction),best.w);}"));
   assert!(s.contains("value=vec4<f32>(gemTrace(origin,direction,random),h.distance);"));
   if !depth {let graph=super::diagnostic_call_graph(&s).unwrap();
   assert!(graph["fs_retained"]["reachable_functions"].as_array().unwrap().iter().any(|v|v=="workstationBackdrop"));}
  }
  let unchanged=super::source(original,true,false,true).unwrap();assert!(unchanged.contains("value=vec4<f32>(gemTrace(origin,direction,random),1e19);"));
 }
}

#[cfg(test)]mod transport_layer_tests {
 #[test]fn original_button_radiance_is_separate_and_pressure_does_not_trace(){
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  let flat=super::super::super::backdrop::source(original).unwrap();
  for compact in [false,true]{let s=super::source(&flat,true,false,compact).unwrap();
   assert!(s.contains("const retainedTransportButtons:bool=true;"));
   assert!(s.contains("retainedGroup=select(group,6,retainedTransportButtons&&group==5)"));
   assert!(s.contains("let captured=retainedMeta.rows[53u+button]"));
   assert!(s.contains("let current=gemRow(192+i32(button)*2)"));
   assert!(s.contains("v=vec4<f32>(v.xyz,gemRow(1).z-(current.z+localZ*current.w))"));
   assert!(!s.contains("10.0*pressure"));assert!(!s.contains("v.xyz*(1.0-0.14"));
   super::diagnostic_call_graph(&s).unwrap();
  }
 }
}

/// Preserve the canonical shader and add a cheaper entry for pixels outside
/// conservative meter scissors. The original entry is redrawn inside them.
pub(super) fn append_meter_base(s:&mut String)->Result<(),String>{
 let start=s.find("@fragment fn fs_retained(").ok_or("missing retained fragment")?;
 let open=start+s[start..].find('{').ok_or("missing retained body")?;let mut depth=0i32;let mut end=None;
 for(i,c)in s[open..].char_indices(){if c=='{'{depth+=1;}else if c=='}'{depth-=1;if depth==0{end=Some(open+i+1);break;}}}
 let mut base=s[start..end.ok_or("unterminated retained fragment")?].to_string();
 let a=base.find("  if(gemRow(43152).w>0.5){").ok_or("missing canonical meter gate")?;
 let b=base[a..].find("  let mark=gemTimelinePlayhead").ok_or("missing post-meter playhead")?+a;
 base.replace_range(a..b,"");base=base.replacen("fn fs_retained(","fn fs_retained_without_meters(",1);
 s.push('\n');s.push_str(&base);Ok(())
}
#[cfg(test)]mod meter_partition_tests{
 #[test]fn offline_fader_is_drawn_in_the_base_pass_and_meter_overlay(){
  if !super::offline_faders_enabled(){return;}
  let mut s=super::source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,true).unwrap();
  super::append_meter_base(&mut s).unwrap();
  assert_eq!(s.matches("best=retainedFaderCap(origin,best);").count(),2);
 }
 #[test]fn original_fragment_is_preserved_and_base_excludes_only_live_meter_loop(){
  let original=super::source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,true).unwrap();let mut s=original.clone();super::append_meter_base(&mut s).unwrap();assert_eq!(&s[..original.len()],original);let extra=&s[original.len()..];assert!(!extra.contains("retainedMeterTrace"));assert!(!extra.contains("gemMixerTraceInstance"));assert!(extra.contains("gemBottomSurfaceHit"));assert!(extra.contains("gemTimelinePlayhead"));assert!(extra.contains("gemDisplay(total*0.5)"));assert!(extra.contains("hasForeground"));let m=naga::front::wgsl::parse_str(&s).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();
 }
}

#[cfg(test)] mod density_zero_guard_tests {
 #[test] fn full_production_source_keeps_certificate_through_live_ledger_and_incremental_stages(){
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  let source=super::source(original,true,false,true).unwrap();
  let certificate=super::density_zero_guard::source(&source).unwrap();
  let ledger=super::with_meter_ledger(&certificate).unwrap();
  let tiled=super::super::incremental::tiled_source(&ledger).unwrap();
  assert!(tiled.contains("DENSITY_ZERO_CERTIFICATE_V1"));
  assert!(tiled.contains("DENSITY_MIP_INTEGER_CERTIFICATE_V1"));
  assert!(tiled.contains("fn retainedMeterTrace("));
 }
 #[test] fn uncertified_header_and_lod_keep_original_sampler_loop(){
  let source=super::density_zero_guard::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
  assert!(source.contains("if(edge>=128&&edge<=16384&&(edge&(edge-1))==0&&level>=0&&level<=7"));
  assert!(source.contains("offset+=edge*edge;edge/=2;"));
 }
}

#[cfg(test)] mod zero_alpha_light_tests {
 #[test] fn full_production_source_keeps_guard_through_live_ledger_and_incremental_stages(){
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");
  let source=super::source(original,true,false,true).unwrap();
  let certificate=super::zero_alpha_light::transform(&source,true).unwrap();
  let ledger=super::with_meter_ledger(&certificate).unwrap();
  let tiled=super::super::incremental::tiled_source(&ledger).unwrap();
  assert!(tiled.contains("ZERO_ALPHA_IO_LIGHT_V1"));
  assert!(tiled.contains("fn retainedMeterTrace("));
  assert_eq!(super::zero_alpha_light::transform(&source,false).unwrap(),source);
 }
}

#[cfg(test)] mod primary_witness_tests {
 #[test] fn generated_primary_witness_survives_ledger_and_incremental_stages(){
  let raw=include_str!("../Procedures/Shaders/workstation.wgsl");
  let plain=super::super::super::backdrop::source(raw).unwrap();
  let baseline=super::source(&plain,true,false,true).unwrap();
  let candidate=super::primary_witness::transform(&baseline,true).unwrap();
  let ledger=super::with_meter_ledger(&candidate).unwrap();
  let tiled=super::super::incremental::tiled_source(&ledger).unwrap();
  assert!(tiled.contains("// PRIMARY_WITNESS_V1"));
  assert_eq!(super::primary_witness::transform(&baseline,false).unwrap(),baseline);
  let module=naga::front::wgsl::parse_str(&tiled).unwrap();
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
 }
}

#[cfg(test)] mod primary_group_gates_tests {
 #[test] fn actual_production_group_gates_validate_with_ledger_and_incremental(){
  let raw=include_str!("../Procedures/Shaders/workstation.wgsl");
  let plain=super::super::super::backdrop::source(raw).unwrap();
  let original=super::source(&plain,true,false,true).unwrap();
  let gated=super::primary_group_gates::transform(&original,true,include_str!("WorkstationPrerender.rs"),include_str!("WorkstationIncrementalBake.rs"),include_str!("../Workstation/DecorativeControls.rs")).unwrap();
  assert!(gated.contains("// PRIMARY_GROUP_GATES_V1"));
  let ledger=super::with_meter_ledger(&gated).unwrap();
  let tiled=super::super::incremental::tiled_source(&ledger).unwrap();
  let module=naga::front::wgsl::parse_str(&tiled).unwrap();
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
 }
}
