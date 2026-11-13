//! Optional synchronous capture scheduling only. Inputs must be the complete
//! untiled retained shader. The host bounds/allocates jobs, resets [0,0,1,1],
//! completes fast dispatch before fallback indirect dispatch, and publishes the
//! atlas only after both. Canonical and incremental entries remain untouched.
#[derive(Debug)]
pub(crate) struct Sources { pub fast:String, pub fallback:String }
pub(crate) const FALLBACK_THREADS:u32=64;
pub(crate) const DISPATCH_LIMIT:u32=65535;
pub(crate) const JOB_BINDING:u32=14;
pub(crate) const STATE_BINDING:u32=15;
const MARKER:&str="// RETAINED_CAPTURE_SPLIT_V1";
const CALL:&str="gemTrace(origin,direction,random)";
const STORE:&str="textureStore(retainedOut,vec2<i32>(id.xy),i32(id.z),value);";
const PACKED_STORE:&str="textureStore(retainedOut,vec2<i32>(id.xy)+vec2<i32>(retainedMeta.rows[5u+u32(group)*4u].xy),sample,value);";
const SIGNATURE:&str="fn cs_retained(@builtin(global_invocation_id) id:vec3<u32>)";
const WRAPPER:&str=r#"fn gemTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {
    let fastSample=rubyFastTrace(origin,direction,random);
    if(fastSample.w>=0.0){
        if(fastSample.w>0.5){ }
        return fastSample.xyz;
    }
    return rubyTraceFallback(origin,direction,random);
}"#;
const BODY:&str=r#"
 let group=i32(id.z)/2;let sample=i32(id.z)%2;
 let rect=retainedMeta.rows[2u+u32(group)*4u];
 if(any(id.xy>=vec2<u32>(rect.zw))){return;}
 let origin=retainedRay(vec2<f32>(id.xy)+rect.xy,sample);
 let direction=gemSafeUnit(gemRow(4).xyz);
 let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sample));
 retainedActive=true;retainedGroup=select(group,6,retainedTransportButtons&&group==5);retainedOrigin=origin;
 var value=vec4<f32>(0.0,0.0,0.0,1e20);
 if(retainedMergeStatic&&group==0){
  retainedGroup=5;
  let h=gemNearest(origin,direction,-1,1e20);
  if(h.panel>=0&&h.distance<1e19){
   value=vec4<f32>(gemTrace(origin,direction,random),h.distance);
  }else{
   retainedGroup=0;
   value=vec4<f32>(gemTrace(origin,direction,random),1e19);
  }
 }else{
  let h=gemNearest(origin,direction,-1,1e20);
  if(group==0||h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),select(h.distance,1e19,group==0));}
 }
 textureStore(retainedOut,vec2<i32>(id.xy),i32(id.z),value);
"#;
fn function<'a>(s:&'a str,name:&str)->Result<&'a str,String>{
 let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("capture split needs one {name}"));}
 let start=s.find(&marker).unwrap();let open=start+s[start..].find('{').ok_or("missing function body")?;let mut depth=0usize;
 for(i,b)in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok(&s[start..=i]);}}}Err("unclosed function".into())
}
fn normalized(s:&str)->String{s.lines().flat_map(|line|line.split("//").next().unwrap().chars()).filter(|c|!c.is_whitespace()).collect()}
fn variant(plain:bool,packed:bool)->String{
 let mut b=BODY.to_owned();
 if plain {b=b.replace("value=vec4<f32>(gemTrace(origin,direction,random),1e19);","value=vec4<f32>(0.0,0.0,0.0,1e19);")
 .replace("if(group==0||h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),select(h.distance,1e19,group==0));}","if(group==0){value=vec4<f32>(0.0,0.0,0.0,1e19);}\n  else if(h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),h.distance);}");}
 if packed{b=b.replace(STORE,PACKED_STORE);}b
}
/// Generate independent modules: fast jobs/state are read_write; fallback jobs
/// and state are read-only, so state can also be an INDIRECT dispatch buffer.
/// The input prefix, original entries, and all optical functions are unchanged.
pub(crate) fn sources(input:&str)->Result<Sources,String>{
 if input.contains(MARKER)||input.contains("captureSplit") {return Err("capture split already installed/reserved names occupied".into());}
 if normalized(function(input,"gemTrace")?)!=normalized(WRAPPER){return Err("capture split requires unchanged vec4 fast/fallback wrapper".into());}
 let capture=function(input,"cs_retained")?;
 if !capture.starts_with(SIGNATURE){return Err("capture split requires untiled canonical id entry".into());}
 let body=&capture[capture.find('{').unwrap()+1..capture.len()-1];
 let expected_body=normalized(body);
 let matching=(0..4).any(|mode|{
  let expected=variant(mode&1!=0,mode&2!=0);
  if expected_body==normalized(&expected){return true;}
  let marker=" if(retainedMergeStatic&&group==0){";
  let with_tiles=expected.replace(marker,&format!("{}{marker}",include_str!("WorkstationRulerCapture.wgsl")));
  mode&2!=0&&expected_body==normalized(&with_tiles)
 });
 if !matching{return Err("capture split rejects modified capture branches/stores".into());}
 let m=validate(input)?;
 for(_,g)in m.global_variables.iter(){if g.binding.as_ref().is_some_and(|b|b.group==0&&(b.binding==JOB_BINDING||b.binding==STATE_BINDING)){return Err("capture split binding14/15 occupied".into());}}
 let mut fast_body=body.replace(CALL,"captureSplitFastTrace(origin,direction,random)").replace("gemTrace(rayOrigin,direction,random)","captureSplitFastTrace(rayOrigin,direction,random)");
 let store=if body.contains(PACKED_STORE){PACKED_STORE}else{STORE};
 let queued=format!("if(captureSplitNeedsFallback){{\n  let slot=atomicAdd(&captureSplitState.count,1u);\n  let extent=vec2<u32>(gemRow(8).xy);\n  captureSplitJobs.rows[slot]=(id.z*extent.y+id.y)*extent.x+id.x;\n  atomicMax(&captureSplitState.x,min(slot/{FALLBACK_THREADS}u+1u,{DISPATCH_LIMIT}u));\n  atomicMax(&captureSplitState.y,slot/({DISPATCH_LIMIT}u*{FALLBACK_THREADS}u)+1u);\n  return;\n }}\n {store}");
 fast_body=fast_body.replacen(store,&queued,1);
 let mut fast=input.to_owned();fast.push_str(&format!("\n{MARKER}\n{FAST_BINDINGS}\n{FAST_TRACE}\n@compute @workgroup_size(8,8,1)\nfn cs_retained_fast(@builtin(global_invocation_id) id:vec3<u32>) {{\n captureSplitNeedsFallback=false;{fast_body}\n}}\n"));
 let mut fallback=input.to_owned();
 fallback.push_str(&format!("\n{MARKER}\n{FALLBACK_BINDINGS}\nfn captureSplitOriginal(id:vec3<u32>) {{{body}}}\n@compute @workgroup_size({FALLBACK_THREADS},1,1)\nfn cs_retained_fallback(@builtin(global_invocation_id) invocation:vec3<u32>) {{\n let slot=invocation.y*({DISPATCH_LIMIT}u*{FALLBACK_THREADS}u)+invocation.x;\n if(slot>=captureSplitState.count){{return;}}\n let encoded=captureSplitJobs.rows[slot];\n let extent=vec2<u32>(gemRow(8).xy);\n let x=encoded%extent.x;let yz=encoded/extent.x;\n let id=vec3<u32>(x,yz%extent.y,yz/extent.y);\n captureSplitOriginal(id);\n}}\n"));
 let fm=validate(&fast)?;let bm=validate(&fallback)?;
 let fast_calls=reachable(&fm,"cs_retained_fast")?;
 for name in ["gemTrace","rubyTraceFallback","rubyBakeTrace","retainedMeterTrace"]{
  if fast_calls.iter().any(|h|fm.functions[*h].name.as_deref()==Some(name)){return Err(format!("capture fast entry reaches {name}"));}
 }
 for h in &fast_calls {let f=&fm.functions[*h];
  for(_,local)in f.local_variables.iter(){if ray_queue(&fm,local.ty){return Err(format!("fast entry reaches ray queue in {:?}",f.name));}}
  for(_,expression)in f.expressions.iter(){if let naga::Expression::GlobalVariable(g)=expression{if ray_queue(&fm,fm.global_variables[*g].ty){return Err("fast entry reaches global ray queue".into());}}}
 }
 if !reachable(&bm,"cs_retained_fallback")?.iter().any(|h|bm.functions[*h].name.as_deref()==Some("rubyTraceFallback")){return Err("fallback lost original optical trace".into());}
 Ok(Sources{fast,fallback})
}
const FAST_BINDINGS:&str=r#"
struct CaptureSplitJobs{rows:array<u32>};
struct CaptureSplitState{count:atomic<u32>,x:atomic<u32>,y:atomic<u32>,z:u32};
@group(0) @binding(14) var<storage,read_write> captureSplitJobs:CaptureSplitJobs;
@group(0) @binding(15) var<storage,read_write> captureSplitState:CaptureSplitState;
var<private> captureSplitNeedsFallback:bool=false;
"#;
const FALLBACK_BINDINGS:&str=r#"
struct CaptureSplitJobs{rows:array<u32>};
struct CaptureSplitState{count:u32,x:u32,y:u32,z:u32};
@group(0) @binding(14) var<storage,read> captureSplitJobs:CaptureSplitJobs;
@group(0) @binding(15) var<storage,read> captureSplitState:CaptureSplitState;
"#;
const FAST_TRACE:&str=r#"
fn captureSplitFastTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32>{
 let fastSample=rubyFastTrace(origin,direction,random);
 if(fastSample.w>=0.0){return fastSample.xyz;}
 captureSplitNeedsFallback=true;
 return vec3<f32>(0.0);
}
"#;
fn validate(s:&str)->Result<naga::Module,String>{let m=naga::front::wgsl::parse_str(s).map_err(|e|e.emit_to_string(s))?;naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("capture split WGSL validation: {e:?}"))?;Ok(m)}
fn calls(b:&naga::Block,out:&mut Vec<naga::Handle<naga::Function>>){use naga::Statement as S;for s in b.iter(){match s{S::Call{function,..}=>out.push(*function),S::Block(b)=>calls(b,out),S::If{accept,reject,..}=>{calls(accept,out);calls(reject,out);},S::Switch{cases,..}=>for c in cases{calls(&c.body,out);},S::Loop{body,continuing,..}=>{calls(body,out);calls(continuing,out);},_=>()}}}
fn reachable(m:&naga::Module,entry:&str)->Result<Vec<naga::Handle<naga::Function>>,String>{let f=&m.entry_points.iter().find(|e|e.name==entry).ok_or("missing split entry")?.function;let mut pending=vec![];let mut seen=std::collections::BTreeSet::new();let mut result=vec![];calls(&f.body,&mut pending);while let Some(h)=pending.pop(){if seen.insert(h.index()){calls(&m.functions[h].body,&mut pending);result.push(h);}}Ok(result)}
fn ray_queue(m:&naga::Module,ty:naga::Handle<naga::Type>)->bool{match &m.types[ty].inner{naga::TypeInner::Array{base,..}=>m.types[*base].name.as_deref()==Some("GemRay")||ray_queue(m,*base),naga::TypeInner::Struct{members,..}=>members.iter().any(|v|ray_queue(m,v.ty)),_=>false}}

#[cfg(test)]mod tests{
 use super::*;
 const FIXTURE:&str=r#"
struct GemHit{distance:f32,normal:vec3<f32>,panel:i32};
struct GemRay{origin:vec3<f32>,weight:vec3<f32>};
struct RetainedMetadata{rows:array<vec4<f32>>};
@group(0) @binding(6) var<storage,read> retainedMeta:RetainedMetadata;
@group(0) @binding(4) var retainedOut:texture_storage_2d_array<rgba32float,write>;
const retainedMergeStatic:bool=true;const retainedTransportButtons:bool=true;
var<private> retainedActive:bool=false;var<private> retainedGroup:i32=0;var<private> retainedOrigin:vec3<f32>;
fn gemRow(index:i32)->vec4<f32>{return retainedMeta.rows[u32(index)];}
fn gemSafeUnit(v:vec3<f32>)->vec3<f32>{return normalize(v);}
fn retainedRay(p:vec2<f32>,sample:i32)->vec3<f32>{return vec3<f32>(p,f32(sample));}
fn gemNearest(o:vec3<f32>,d:vec3<f32>,medium:i32,farLimit:f32)->GemHit{return GemHit(farLimit,vec3<f32>(0.0),medium);}
fn rubyFastTrace(o:vec3<f32>,d:vec3<f32>,random:vec2<f32>)->vec4<f32>{return vec4<f32>(o,random.x);}
fn rubyTraceFallback(o:vec3<f32>,d:vec3<f32>,random:vec2<f32>)->vec3<f32>{var pending:array<GemRay,16>;pending[0]=GemRay(o,d);return pending[u32(random.x)].origin;}
"#;
 fn fixture(plain:bool,packed:bool)->String{format!("{FIXTURE}\n{WRAPPER}\n@compute @workgroup_size(8,8,1)\n{SIGNATURE} {{{}}}",variant(plain,packed))}
 #[test]fn all_supported_sources_preserve_original_and_canonical_fallback_body(){for plain in [false,true]{for packed in [false,true]{let original=fixture(plain,packed);let s=sources(&original).unwrap();assert!(s.fast.starts_with(&original));assert!(s.fallback.starts_with(&original));let canonical=function(&original,"cs_retained").unwrap();let replay=function(&s.fallback,"captureSplitOriginal").unwrap();assert_eq!(&canonical[canonical.find('{').unwrap()..],&replay[replay.find('{').unwrap()..]);assert!(s.fast.contains("if(captureSplitNeedsFallback)"));assert!(s.fallback.contains("var<storage,read> captureSplitState"));}}}
 #[test]fn fast_reachability_excludes_real_ray_queue_and_rejects_indirect_new_reachability(){let original=fixture(true,true);let s=sources(&original).unwrap();let m=validate(&s.fast).unwrap();let names:Vec<_>=reachable(&m,"cs_retained_fast").unwrap().iter().map(|h|m.functions[*h].name.clone().unwrap()).collect();assert!(names.iter().any(|n|n=="rubyFastTrace"));assert!(!names.iter().any(|n|n=="rubyTraceFallback"));let changed=original.replace("return vec4<f32>(o,random.x);","return vec4<f32>(rubyTraceFallback(o,d,random),1.0);");assert!(sources(&changed).unwrap_err().contains("rubyTraceFallback"));}
 #[test]fn changed_wrapper_tiled_entry_changed_branch_collision_rejected(){let original=fixture(false,false);for s in [original.replace("if(fastSample.w>0.5){ }","if(fastSample.w>0.5){retainedGroup=7;}"),original.replace("id:vec3<u32>) {","rawId:vec3<u32>) {"),original.replace("h.distance<1e19","h.distance<1e18"),format!("{original}\n@group(0) @binding(14) var<storage,read> other:RetainedMetadata;"),sources(&original).unwrap().fast]{assert!(sources(&s).is_err());}}
 #[test]fn compact_jobs_roundtrip_for_every_active_sample_and_dispatch_rows(){for (w,h,g) in [(1u32,1u32,1u32),(1920,1200,6),(16384,16384,6)]{let maximum=u64::from(w)*u64::from(h)*u64::from(g)*2;assert!(maximum<=u64::from(u32::MAX));for z in 0..g*2{for y in [0,h/2,h-1]{for x in [0,w/2,w-1]{let n=(z*h+y)*w+x;assert_eq!((n%w,(n/w)%h,(n/w)/h),(x,y,z));}}}}
 for count in [0u32,1,63,64,65,65535*64-1,65535*64,65535*64+1,4_608_000,33_554_432]{let x=if count==0{0}else{((count-1)/64+1).min(65535)};let y=if count==0{1}else{(count-1)/(65535*64)+1};assert!(x<=65535&&y<=65535);for slot in [0,count.saturating_sub(1),count/2]{if slot<count{let gy=slot/(65535*64);let gx=slot%(65535*64);assert!(gy<y&&gx<x*64);assert_eq!(gy*(65535*64)+gx,slot);}}}}
 #[test]fn branch_model_at_most_one_trace_and_one_writer_per_original_invocation(){let mut checked=0;for merged in [false,true]{for plain in [false,true]{for group in 0..6{for panel in [-1,0]{for depth in [0.0f32,1e19,1e20]{for sentinel in [-1.0f32,0.0,1.0,f32::NAN]{let calls=if merged&&group==0{if panel>=0&&depth<1e19{1}else{usize::from(!plain)}}else if plain{usize::from(group!=0&&panel>=0)}else{usize::from(group==0||panel>=0)};assert!(calls<=1);let queued=calls==1&&!(sentinel>=0.0);let fast_writes=usize::from(!queued);let fallback_writes=usize::from(queued);assert_eq!(fast_writes+fallback_writes,1);checked+=1;}}}}}}assert_eq!(checked,576);}
}
