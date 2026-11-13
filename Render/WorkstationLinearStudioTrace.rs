//! Atlas-only opt-in: studio terminal rays finish outside, so normal ruby
//! fallback retains one continuation. Keep the untouched full fallback and
//! restart it from original arguments if any event retains BOTH branches.
fn span(s:&str,name:&str)->Result<(usize,usize),String>{
 let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("linear studio function changed: {name}"));}
 let start=s.find(&marker).unwrap();let open=start+s[start..].find('{').ok_or("missing linear studio body")?;
 Ok((start,block_end(s,open)?))
}
fn block_end(s:&str,open:usize)->Result<usize,String>{
 let mut depth=0;for(i,c)in s[open..].char_indices(){if c=='{'{depth+=1;}else if c=='}'{depth-=1;if depth==0{return Ok(open+i+1);}}}
 Err("unclosed linear studio block".into())
}
fn once(s:&mut String,a:&str,b:&str)->Result<(),String>{
 if s.matches(a).count()!=1{return Err(format!("linear studio marker changed: {a}"));}*s=s.replacen(a,b,1);Ok(())
}
fn compact(s:&str)->String{s.chars().filter(|c|!c.is_whitespace()).collect()}
const TERMINAL:&str=r#"fn gemTerminal(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32> {
    // Exterior policy: resolve the studio directly without scene occlusion.
    let environment=gemEnvironment(origin,ray);
    var radiance=environment.xyz;
    if(radiance.x<0.0){radiance=gemFloor(origin+ray*environment.w,-ray);}
    return vec4<f32>(radiance,1.0);
}"#;
const QUEUE:&str="    var pending:array<GemRay,16>;var count:i32=0;\n";
const SELECT:&str=r#"        // Spend the finite ray budget on the strongest remaining throughput.
        // Depth-first transmission could exhaust the budget in a dark branch
        // while the primary reflected card was still waiting on the stack.
        var strongest:i32=-1;var strongestWeight:f32=current.priority;
        for(var candidate:i32=0;candidate<16;candidate++){
            if(candidate>=count){break;}
            var weight:f32=pending[candidate].priority;
            if(weight>strongestWeight){strongest=candidate;strongestWeight=weight;}
        }
        if(strongest>=0){var swapRay:GemRay=current;current=pending[strongest];pending[strongest]=swapRay;}
"#;
const BOTH:&str="if(retainReflection&&retainTransmission){if(reflectPriority>transmitPriority){current=reflection;if(count<16){pending[count]=transmission;count++;}}else{current=transmission;if(count<16){pending[count]=reflection;count++;}}}";
const FINISH:&str="        if(finished){\n            if(count==0){break;}count--;current=pending[count];\n        }";
fn linear_trace(reference:&str)->Result<String,String>{
 let primary=reference.contains("random:vec2<f32>,primary:RubyPrimaryResult)->vec3<f32>");
 if !primary&&!reference.contains("random:vec2<f32>)->vec3<f32>"){return Err("unknown fallback arguments".into());}
 for invariant in ["var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);","for(var event:i32=0;event<64;event++)","if(f32(event)>=gemRow(8).w){break;}"]{
  if reference.matches(invariant).count()!=1{return Err(format!("linear studio initialization changed: {invariant}"));}
 }
 let mut result=reference.to_owned();once(&mut result,"fn rubyTraceFallback(","fn rubyTraceLinearStudio(")?;
 once(&mut result,QUEUE,"")?;once(&mut result,SELECT,"")?;
 let marker="if(retainReflection&&retainTransmission){";
 let mut positions=Vec::new();let mut cursor=0;
 while let Some(offset)=result[cursor..].find(marker){
  let start=cursor+offset;let end=block_end(&result,start+marker.len()-1)?;
  if compact(&result[start..end])!=BOTH{return Err("linear studio branch scheduling changed".into());}
  positions.push((start,end));cursor=end;
 }
 if positions.len()!=2{return Err("expected membrane and dielectric branch sites".into());}
 let args=if primary{"origin,direction,random,primary"}else{"origin,direction,random"};
 let escape=format!("if(retainReflection&&retainTransmission){{return rubyTraceFallback({args});}}");
 for(start,end)in positions.into_iter().rev(){result.replace_range(start..end,&escape);}
 once(&mut result,FINISH,"        if(finished){break;}")?;
 if result.contains("pending")||result.split(|c:char|!c.is_alphanumeric()&&c!='_').any(|w|w=="count"){return Err("queue state escaped linear rewrite".into());}
 Ok(result)
}
pub(super)fn source(original:&str)->Result<String,String>{
 if original.contains("fn rubyTraceLinearStudio("){return Err("linear studio already installed".into());}
 let(a,b)=span(original,"gemTerminal")?;
 if &original[a..b]!=TERMINAL{return Err("linear studio requires original always-complete studio terminal".into());}
 let(a,b)=span(original,"rubyTraceFallback")?;let trace=linear_trace(&original[a..b])?;
 let(a,b)=span(original,"gemTrace")?;let mut wrapper=original[a..b].to_owned();
 once(&mut wrapper,"return rubyTraceFallback(","return rubyTraceLinearStudio(")?;
 let mut result=original.to_owned();result.replace_range(a..b,&wrapper);result.push('\n');result.push_str(&trace);result.push('\n');Ok(result)
}

// The only reachable private write in the retained capture call graph is a
// scoped primary-visibility flag. Its exact wrapper recomputes it from immutable
// invocation inputs, and clears it before returning. No bake counters, storage
// writes, or other persistent side effects may precede a restart.
const NEAREST:&str=r#"fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {
 let primary=retainedActive&&medium<0&&all(origin==retainedOrigin);
 retainedPrimary=primary;
 let hit=retainedOriginalNearest(origin,ray,medium,farLimit);
 retainedPrimary=false;
 return hit;
}"#;
pub(super)fn validate_restart_safety(s:&str,m:&naga::Module,info:&naga::valid::ModuleInfo)->Result<(),String>{
 let(a,b)=span(s,"gemNearest")?;if &s[a..b]!=NEAREST{return Err("linear studio primary scope changed".into());}
 for name in ["rubyTraceFallback","rubyTraceLinearStudio"]{
  let(handle,_)=m.functions.iter().find(|(_,f)|f.name.as_deref()==Some(name)).ok_or("missing linear restart function")?;
  for(global,v)in m.global_variables.iter(){
   if info[handle][global].contains(naga::valid::GlobalUse::WRITE)&&v.name.as_deref()!=Some("retainedPrimary"){
    return Err(format!("linear restart has persistent write: {name} -> {:?}",v.name));
   }
  }
 }
 // Require gemNearest to be the sole direct writer of retainedPrimary. This
 // protects against an added helper mutating that same permitted global.
 let retained=m.global_variables.iter().find(|(_,v)|v.name.as_deref()==Some("retainedPrimary")).ok_or("missing primary flag")?.0;
 fn root(f:&naga::Function,p:naga::Handle<naga::Expression>)->Option<naga::Handle<naga::GlobalVariable>>{match f.expressions[p]{naga::Expression::GlobalVariable(g)=>Some(g),naga::Expression::Access{base,..}|naga::Expression::AccessIndex{base,..}=>root(f,base),_=>None}}
 fn writes(b:&naga::Block,f:&naga::Function,g:naga::Handle<naga::GlobalVariable>)->bool{b.iter().any(|s|match s{
  naga::Statement::Store{pointer,..}=>root(f,*pointer)==Some(g),naga::Statement::Block(b)=>writes(b,f,g),
  naga::Statement::If{accept,reject,..}=>writes(accept,f,g)||writes(reject,f,g),naga::Statement::Switch{cases,..}=>cases.iter().any(|c|writes(&c.body,f,g)),
  naga::Statement::Loop{body,continuing,..}=>writes(body,f,g)||writes(continuing,f,g),_=>false
 })}
 for(_,f)in m.functions.iter(){if f.name.as_deref()!=Some("gemNearest")&&writes(&f.body,f,retained){return Err("unexpected retainedPrimary writer".into());}}
 Ok(())
}

#[cfg(test)]mod tests{
 use super::*;use naga::Statement;
 fn original()->String{crate::workstation::ruby_response_shader::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap()}
 fn body<'a>(s:&'a str,name:&str)->&'a str{let(a,b)=span(s,name).unwrap();&s[a..b]}
 fn validated(s:&str)->naga::Module{let m=naga::front::wgsl::parse_str(s).unwrap_or_else(|e|panic!("{}",e.emit_to_string(s)));naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();m}
 fn calls(block:&naga::Block,m:&naga::Module,out:&mut Vec<String>){for s in block{match s{
  Statement::Call{function,..}=>out.push(m.functions[*function].name.clone().unwrap()),Statement::Block(b)=>calls(b,m,out),
  Statement::If{accept,reject,..}=>{calls(accept,m,out);calls(reject,m,out);},Statement::Switch{cases,..}=>for c in cases{calls(&c.body,m,out)},
  Statement::Loop{body,continuing,..}=>{calls(body,m,out);calls(continuing,m,out)},_=>()
 }}}
 #[test]fn full_shader_validates_queue_removed_and_escapes_are_nonrecursive(){
  let s=source(&original()).unwrap();let m=validated(&s);
  let f=m.functions.iter().find(|(_,f)|f.name.as_deref()==Some("rubyTraceLinearStudio")).unwrap().1;
  assert!(!f.local_variables.iter().any(|(_,v)|matches!(v.name.as_deref(),Some("pending"|"count"|"strongest"|"strongestWeight"))));
  let mut c=Vec::new();calls(&f.body,&m,&mut c);assert_eq!(c.iter().filter(|n|*n=="rubyTraceFallback").count(),2);assert!(!c.iter().any(|n|n=="rubyTraceLinearStudio"||n=="gemTrace"));
  let f=m.functions.iter().find(|(_,f)|f.name.as_deref()==Some("rubyTraceFallback")).unwrap().1;let mut c=Vec::new();calls(&f.body,&m,&mut c);assert!(!c.iter().any(|n|n=="rubyTraceLinearStudio"||n=="gemTrace"||n=="rubyTraceFallback"));
 }
 #[test]fn original_functions_and_all_accumulations_are_unchanged(){
  let before=original();let after=source(&before).unwrap();let m=validated(&before);
  for(_,f)in m.functions.iter(){let n=f.name.as_ref().unwrap();if n!="gemTrace"{assert_eq!(body(&before,n),body(&after,n),"{n}");}}
  let expressions=|s:&str|s.match_indices("result+=").map(|(i,_)|s[i..i+s[i..].find(';').unwrap()+1].to_owned()).collect::<Vec<_>>();
  let old=body(&before,"rubyTraceFallback");let new=body(&after,"rubyTraceLinearStudio");
  // Includes the existing optional cached response addition as well as all
  // original canonical additions; do not silently drop either.
  assert_eq!(expressions(old),expressions(new));assert_eq!(expressions(old).len(),18);
  assert!(new.contains("for(var event:i32=0;event<64;event++)"));assert!(new.contains("if(f32(event)>=gemRow(8).w){break;}"));
  assert_eq!(new.matches("return rubyTraceFallback(origin,direction,random)").count(),2);
 }
 #[test]fn retained_meter_trace_and_meter_ledger_are_unchanged(){
  let plain=super::super::super::super::backdrop::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
  let old=super::super::source(&plain,true,false,true).unwrap();let new=source(&old).unwrap();validated(&new);
  assert_eq!(body(&old,"retainedMeterTrace"),body(&new,"retainedMeterTrace"));
  let old=super::super::with_meter_ledger(&old).unwrap();let new=super::super::with_meter_ledger(&new).unwrap();validated(&new);
  for n in ["retainedMeterTrace","meterLedgerCaptureTrace","cs_meter_ledger_capture","fs_retained"]{assert_eq!(body(&old,n),body(&new,n),"{n}");}
 }
 #[test]fn unsupported_terminal_or_branch_changes_and_repeat_application_fail_closed(){
  let old=original();assert!(source(&source(&old).unwrap()).is_err());
  assert!(source(&old.replace("return vec4<f32>(radiance,1.0);","return vec4<f32>(radiance,0.0);")).is_err());
  assert!(source(&old.replacen("if(count<16){pending[count]=transmission;count++;}","if(count<15){pending[count]=transmission;count++;}",1)).is_err());
 }
 #[test]fn branching_restart_uses_only_original_arguments_with_primary_payload(){
  // The real primary-reuse transform is independently applied in the harness
  // configuration as well. This test verifies parameter forwarding directly.
  let old=original();let trace=body(&old,"rubyTraceFallback");
  let payload=trace.replacen("random:vec2<f32>)->vec3<f32>","random:vec2<f32>,primary:RubyPrimaryResult)->vec3<f32>",1);
  let new=linear_trace(&payload).unwrap();assert_eq!(new.matches("return rubyTraceFallback(origin,direction,random,primary)").count(),2);
  assert!(!new.contains("rubyTraceFallback(current."));
 }
 #[test]fn restart_has_only_scoped_primary_flag_write_and_rejects_new_side_effects(){
  let plain=super::super::super::super::backdrop::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
  let s=source(&super::super::source(&plain,true,false,true).unwrap()).unwrap();
  let check=|s:&str|{let m=validated(s);let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();validate_restart_safety(s,&m,&info)};
  check(&s).unwrap();
  assert!(check(&s.replacen("fn rubyTraceLinearStudio(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {","fn rubyTraceLinearStudio(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> { rubyBakeLeaves+=1;",1)).is_err());
  assert!(check(&s.replace(" retainedPrimary=false;\n return hit;"," return hit;")).is_err());
  assert!(check(&s.replacen("fn retainedOriginalNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {","fn retainedOriginalNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit { retainedPrimary=false;",1)).is_err());
 }
}
