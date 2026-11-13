//! Opt-in known-environment-cutoff witness for the outer atlas primary query.
//! Ordinary traversal bodies remain intact. Only certified copies perform the
//! first wide query; any uncertainty keeps the subsequent original finite query.
pub(crate) const ENV:&str="WORMWOOD_WORKSTATION_PRIMARY_WITNESS";
const MARKER:&str="// PRIMARY_WITNESS_V1\n// PRIMARY_WITNESS_TIMELINE_PREFIX_V2\n// PRIMARY_WITNESS_GENERIC_ONLY_V3";
const CONTRACTS:&[(&str,u64)]=&[
 ("gemRow",0x78c85be3220abcbd),
 ("gemBox",0x25a3c434270b3db4),
 ("gemInverseRay",0xdf64e0a00fad0288),
 ("gemControlNormal",0xbb3c4dd8b28f1e46),
 ("gemSafeUnit",0xbdc19e3da465a4c0),
 ("gemControlPoseRow",0xe41895397afa7b00),
 ("gemPanelRow",0xb67bdee25ec12a9c),
 ("gemMixerScale",0x185504db25f1b663),
 ("gemMixerTranslation",0x91fdc7165121d57d),
 ("gemMixerRecord",0xe7d519d777ef7e19),
 ("gemMixerActive",0xe0cb32472a86854e),
 ("gemMixerTree",0x33050d94ae01dcdc),
 ("gemBarPlane",0x942b5743d06d5342),
 ("gemEnvironment",0x1e3a1f6cd3f88aa2),
 ("gemLightDistance",0x4167420ca1ff8315),
 ("retainedOriginalNearest",0x689864ea3ee995ab),
 ("gemNearest",0xb712ebc68997396a),
 ("retainedControlGroup",0xfd9af09a9cf83835),
 ("gemEnterPanel",0xf4e8a79cf0a457c4),
 ("gemControlHit",0x5274e83b41c06b1b),
 ("gemMixerTraceInstance",0x39992e7b0e7eb002),
 ("gemMixerHit",0x8e5cfb6e2db81c67),
 ("gemBarHit",0x01dc9c9840cca799),
 ("gemTimelineHit",0xc5a98848940e844b),
 ("gemTimelineMesh",0xeef45d4ab9bf7eb3),
 ("gemTimelineMarkerBox",0xdb06bfc0bbc4318f),
 ("gemPluginDropMarker",0x2f0ea4d620a05461),
 ("gemBubbleHit",0xd1bb409590296344),
 ("gemAudioSphereHit",0xf5b0f360001a19c2),
 ("userStyleHit",0x8cebf8618942734f),
 ("gemBottomSurfaceHit",0x6714589dcd468907),
 ("gemTimelinePlayhead",0xe1d50484111ac364),
 ("cs_retained",0x61029132d6921346),
];
fn span(s:&str,n:&str)->Result<(usize,usize),String>{let marker=format!("fn {n}(");if s.matches(&marker).count()!=1{return Err(format!("primary witness requires one {n}"));}let a=s.find(&marker).unwrap();let open=a+s[a..].find('{').ok_or("missing function body")?;let mut depth=0usize;for(i,b)in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok((a,i+1));}}}Err("unclosed witness function".into())}
fn function<'a>(s:&'a str,n:&str)->Result<&'a str,String>{let(a,b)=span(s,n)?;Ok(&s[a..b])}
fn hash(s:&str)->u64{s.lines().flat_map(|l|l.split("//").next().unwrap().bytes()).filter(|b|!b.is_ascii_whitespace()).fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(b)).wrapping_mul(0x100000001b3))}
fn once(s:&mut String,a:&str,b:&str)->Result<(),String>{if s.matches(a).count()!=1{return Err(format!("primary witness marker changed: {a}"));}*s=s.replacen(a,b,1);Ok(())}
const CLONES:&[&str]=&["retainedOriginalNearest","gemEnterPanel","gemControlHit","gemMixerTraceInstance","gemMixerHit","gemBarHit","gemTimelineHit","gemTimelineMesh","gemTimelineMarkerBox","gemPluginDropMarker"];
const UNSUPPORTED:&[&str]=&["gemBubbleHit","gemAudioSphereHit","userStyleHit","gemBottomSurfaceHit","gemTimelinePlayhead"];
fn name(n:&str)->String{format!("primaryWitness_{}",n)}
fn rename(s:&str)->String{let mut out=s.to_owned();for n in CLONES.iter().chain(UNSUPPORTED.iter()){out=out.replace(&format!("{n}("),&format!("{}(",name(n)));}out}
// Only certified clones use this helper. Preserve every original argument and
// append its companion cutoff; never convert a local slab near back to world t.
fn boxes(s:&str)->Result<String,String>{
 let mut out=String::new();let mut cursor=0;
 while let Some(relative)=s[cursor..].find("gemBox("){
  let a=cursor+relative;let open=a+"gemBox".len();let mut depth=0i32;let mut last=open;let mut end=None;
  for(i,b)in s.as_bytes().iter().enumerate().skip(open){match *b{b'('=>depth+=1,b')'=>{depth-=1;if depth==0{end=Some(i);break;}},b',' if depth==1=>last=i,_=>()}}
  let end=end.ok_or("unclosed box call")?;let cap=s[last+1..end].trim();
  let companion=match cap{"farLimit"=>"min(farLimit,primaryWitnessClip)","hit.distance"=>"min(hit.distance,primaryWitnessClip)","hit.distance/pose.w"=>"min(hit.distance,primaryWitnessClip)/pose.w",_=>return Err(format!("unsupported box units: {cap}"))};
  out.push_str(&s[cursor..a]);out.push_str("primaryWitnessBox");out.push_str(&s[open..end]);out.push(',');out.push_str(companion);out.push(')');cursor=end+1;
 }
 out.push_str(&s[cursor..]);Ok(out)
}
fn insert_after_open(s:&mut String,code:&str){let i=s.find('{').unwrap()+1;s.insert_str(i,code);}
fn clone_supported(input:&str,n:&str)->Result<String,String>{
 let mut s=function(input,n)?.to_owned();
 match n{
  "retainedOriginalNearest"=>insert_after_open(&mut s,"\n    if(medium!= -1||!retainedPrimary){primaryWitnessGood=false;}"),
  "gemEnterPanel"=>{
   s=boxes(&s)?;
   once(&mut s,"    if(far<=0.0000001||near<0.0000001||near>farLimit)","    primaryWitnessCut(near,farLimit,false);\n    if(far<=0.0000001||near<0.0000001||near>farLimit)")?;
   once(&mut s,"        var plane:vec4<f32>=gemRow(base+6+i);var denom:f32=dot(plane.xyz,ray);","        var plane:vec4<f32>=gemRow(base+6+i);var denom:f32=dot(plane.xyz,ray);\n        if(!primaryWitnessFinite4(plane)||!primaryWitnessFinite(denom)){primaryWitnessGood=false;}")?;
  },
  "gemControlHit"=>{
   insert_after_open(&mut s,"\n    if(medium!= -1||anyHit){primaryWitnessGood=false;}");s=boxes(&s)?;
   once(&mut s,"        if(pose.w<=0.0){continue;}","        if(pose.w<=0.0){continue;}\n        if(!primaryWitnessFinite4(pose)||!(pose.w>0.0)){primaryWitnessGood=false;}")?;
   once(&mut s,"                var t:f32=dot(e2,q)/det*pose.w;","                var t:f32=dot(e2,q)/det*pose.w;\n                primaryWitnessCut(t,hit.distance,true);")?;
  },
  "gemMixerTraceInstance"=>{
   insert_after_open(&mut s,"\n if(medium!= -1||anyHit){primaryWitnessGood=false;}");s=boxes(&s)?;
   once(&mut s," var tree:vec2<f32>=gemMixerTree(instance);"," if(!primaryWitnessFinite3(scale)||!all(scale>vec3<f32>(0.0))){primaryWitnessGood=false;}\n var tree:vec2<f32>=gemMixerTree(instance);")?;
   once(&mut s,"var t:f32=dot(e2,q)/det;","var t:f32=dot(e2,q)/det;primaryWitnessCut(t,hit.distance,true);")?;
  },
  "gemMixerHit"=>{insert_after_open(&mut s,"\n if(medium!= -1||anyHit){primaryWitnessGood=false;}");s=boxes(&s)?;},
  "gemBarHit"=>{
   insert_after_open(&mut s,"\n    if(medium!= -1){primaryWitnessGood=false;}");
   once(&mut s,"        if(size.w<0.5||pose.w<=0.0){continue;}","        if(size.w<0.5||pose.w<=0.0){continue;}\n        if(!primaryWitnessFinite4(pose)||!primaryWitnessFinite4(size)||!(pose.w>0.0)){primaryWitnessGood=false;}")?;
   once(&mut s,"            var plane:vec4<f32>=gemBarPlane(j,size.xy);var denominator:f32=dot(plane.xyz,ray);","            var plane:vec4<f32>=gemBarPlane(j,size.xy);var denominator:f32=dot(plane.xyz,ray);\n            if(!primaryWitnessFinite4(plane)||!primaryWitnessFinite(denominator)){primaryWitnessGood=false;}")?;
   once(&mut s,"        var worldDistance:f32=t*pose.w;","        var worldDistance:f32=t*pose.w;primaryWitnessCut(worldDistance,hit.distance,true);")?;
  },
  "gemTimelineHit"=>{s=boxes(&s)?;},
  "gemTimelineMesh"=>{
   // Observe only the original initial slab prefix. A complete early miss
   // executes no cutoff-dependent tile selection or bounded BVH traversal.
   once(&mut s,"    if(far<near){return hit;}",r#"    let companionFar=min(min(upper.x,upper.y),min(upper.z,min(farLimit,primaryWitnessClip)/pose.w));
    if(medium!= -1||!primaryWitnessFinite4(pose)||!(pose.w>0.0)
       ||!primaryWitnessFinite3(intervalA)||!primaryWitnessFinite3(intervalB)
       ||!primaryWitnessFinite(farLimit)||!(farLimit>0.0)
       ||!primaryWitnessFinite(near)||!primaryWitnessFinite(far)||!primaryWitnessFinite(companionFar)
       ||((far<near)!=(companionFar<near))){primaryWitnessGood=false;}
    if(far<near){return hit;}
    // Possible intersection: keep original work/output, but never certify any
    // tile-range construction or accepted timeline primitive in this version.
    primaryWitnessGood=false;"#)?;
  },
  "gemTimelineMarkerBox"=>{
   once(&mut s,"    if(near>0.0000001&&far>=near&&near<farLimit)","    if(!primaryWitnessFinite3(a)||!primaryWitnessFinite3(b)){primaryWitnessGood=false;}\n    primaryWitnessCut(near,farLimit,true);\n    if(near>0.0000001&&far>=near&&near<farLimit)")?;
  },
  "gemPluginDropMarker"=>(),_=>return Err("unknown witness clone".into())
 }
 Ok(rename(&s))
}
fn unsupported(input:&str,n:&str)->Result<String,String>{
 let source=function(input,n)?;let open=source.find('{').unwrap();let signature=rename(&source[..open]);
 let(active,args)=match n{
  "gemBubbleHit"=>("!(gemRow(234).x<0.5)","origin,ray,farLimit"),
  "gemAudioSphereHit"=>("!(gemRow(240).x<0.5)","origin,ray,farLimit"),
  "userStyleHit"=>("!(gemRow(245).x<0.5)","origin,ray,farLimit,anyHit,shadow"),
  "gemBottomSurfaceHit"|"gemTimelinePlayhead"=>("true","origin,ray,farLimit"),_=>return Err("unknown unsupported family".into())
 };
 Ok(format!("{signature}{{\n if({active}){{primaryWitnessGood=false;}}\n return {n}({args});\n}}"))
}
fn checked_module(s:&str)->Result<(naga::Module,naga::valid::ModuleInfo),String>{let m=naga::front::wgsl::parse_str(s).map_err(|e|e.emit_to_string(s))?;let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("primary witness validation: {e:?}"))?;Ok((m,info))}
fn pure(m:&naga::Module,info:&naga::valid::ModuleInfo,name:&str)->Result<(),String>{let(h,_)=m.functions.iter().find(|(_,f)|f.name.as_deref()==Some(name)).ok_or("missing audited pure function")?;for(g,v)in m.global_variables.iter(){if info[h][g].contains(naga::valid::GlobalUse::WRITE){return Err(format!("primary witness unsafe original write: {name} -> {:?}",v.name));}}Ok(())}
pub(crate) fn selected(s:&str)->Result<String,String>{transform(s,std::env::var(ENV).as_deref()==Ok("1"))}
pub(crate) fn transform(input:&str,enabled:bool)->Result<String,String>{
 if !enabled{return Ok(input.to_owned());}
 if input.contains("primaryWitness")||input.contains("retainedOuterBegin(")||input.contains("// METER_LEDGER_PLAYBACK_CERTIFICATE"){return Err("primary witness incompatible/duplicate source".into());}
 for(n,expected)in CONTRACTS{if hash(function(input,n)?)!=*expected{return Err(format!("primary witness source contract changed: {n}"));}}
 let(m,info)=checked_module(input)?;pure(&m,&info,"retainedOriginalNearest")?;pure(&m,&info,"gemEnvironment")?;
 let entry=function(input,"cs_retained")?;
 if input.matches("retainedOrigin=origin;").count()!=1||!entry.contains("retainedOrigin=origin;"){return Err("primary witness requires single capture origin assignment".into());}
 // Ensure no other reachable function or entry point can mutate retainedOrigin.
 let origin=m.global_variables.iter().find(|(_,v)|v.name.as_deref()==Some("retainedOrigin")).map(|(g,_)|g).ok_or("missing retained origin")?;
 for(h,f)in m.functions.iter(){if info[h][origin].contains(naga::valid::GlobalUse::WRITE){return Err(format!("primary witness origin write in {:?}",f.name));}}
 for(i,e)in m.entry_points.iter().enumerate(){if e.name!="cs_retained"&&info.get_entry_point(i)[origin].contains(naga::valid::GlobalUse::WRITE){return Err("primary witness origin modified by another entry".into());}}
 let mut out=input.to_owned();let(a,b)=span(&out,"gemNearest")?;out.replace_range(a..b,WRAPPER);
 let(a,b)=span(&out,"cs_retained")?;let entry=&out[a..b];let old="let h=gemNearest(origin,direction,-1,1e20);";
 if entry.matches(old).count()!=2{return Err("primary witness outer query layout changed".into());}
 let entry=entry.replace(old,"let h=primaryWitnessBegin(origin,direction);");out.replace_range(a..b,&entry);
 out.push('\n');out.push_str(MARKER);out.push('\n');out.push_str(SUPPORT);
 let mut b=function(input,"gemBox")?.replace("fn gemBox(","fn primaryWitnessBox(");
 once(&mut b,"farLimit:f32)->bool", "farLimit:f32,companion:f32)->bool")?;
 once(&mut b,"    return min(min(hi.x,hi.y),min(hi.z,farLimit))>=max(max(lo.x,lo.y),max(lo.z,0.0));",r#"    let original=min(min(hi.x,hi.y),min(hi.z,farLimit))>=max(max(lo.x,lo.y),max(lo.z,0.0));
    let other=min(min(hi.x,hi.y),min(hi.z,companion))>=max(max(lo.x,lo.y),max(lo.z,0.0));
    if(!primaryWitnessFinite3(a)||!primaryWitnessFinite3(b)||!primaryWitnessFinite(farLimit)||!primaryWitnessFinite(companion)||!(companion>0.0&&companion<=farLimit)||original!=other){primaryWitnessGood=false;}
    return original;"#)?;
 out.push_str(&b);out.push('\n');
 for n in CLONES{out.push_str(&clone_supported(input,n)?);out.push('\n');}
 for n in UNSUPPORTED{out.push_str(&unsupported(input,n)?);out.push('\n');}
 checked_module(&out)?;Ok(out)
}
const WRAPPER:&str=r#"fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {
 let primary=retainedActive&&medium<0&&all(origin==retainedOrigin);
 retainedPrimary=primary;
 var hit:GemHit;
 if(primaryWitnessAvailable&&primary&&medium== -1&&retainedGroup==primaryWitnessGroup
    &&bitcast<u32>(farLimit)==bitcast<u32>(primaryWitnessClip)
    &&all(bitcast<vec4<u32>>(vec4<f32>(origin,0.0))==bitcast<vec4<u32>>(vec4<f32>(retainedOrigin,0.0)))
    &&all(bitcast<vec4<u32>>(vec4<f32>(ray,0.0))==bitcast<vec4<u32>>(vec4<f32>(primaryWitnessDirection,0.0)))){
  hit=primaryWitnessHit;
 }else{hit=retainedOriginalNearest(origin,ray,medium,farLimit);}
 retainedPrimary=false;
 return hit;
}"#;
const SUPPORT:&str=r#"
var<private> primaryWitnessGood:bool=false;
var<private> primaryWitnessAvailable:bool=false;
var<private> primaryWitnessClip:f32;
var<private> primaryWitnessDirection:vec3<f32>;
var<private> primaryWitnessGroup:i32;
var<private> primaryWitnessHit:GemHit;
fn primaryWitnessFinite(x:f32)->bool{return (bitcast<u32>(x)&0x7f800000u)!=0x7f800000u;}
fn primaryWitnessFinite3(v:vec3<f32>)->bool{return primaryWitnessFinite(v.x)&&primaryWitnessFinite(v.y)&&primaryWitnessFinite(v.z);}
fn primaryWitnessFinite4(v:vec4<f32>)->bool{return primaryWitnessFinite3(v.xyz)&&primaryWitnessFinite(v.w);}
fn primaryWitnessCut(t:f32,limit:f32,strict:bool){
 let companion=min(limit,primaryWitnessClip);
 if(!primaryWitnessFinite(t)||!primaryWitnessFinite(limit)||!primaryWitnessFinite(companion)||!(companion>0.0&&companion<=limit)){primaryWitnessGood=false;return;}
 if(strict){if((t<limit)!=(t<companion)){primaryWitnessGood=false;}}
 else{if((t<=limit)!=(t<=companion)){primaryWitnessGood=false;}}
}
fn primaryWitnessBegin(origin:vec3<f32>,direction:vec3<f32>)->GemHit {
 primaryWitnessAvailable=false;
 let clip=gemEnvironment(origin,direction).w;
 primaryWitnessClip=clip;primaryWitnessDirection=direction;primaryWitnessGroup=retainedGroup;
 primaryWitnessGood=retainedActive&&primaryWitnessFinite3(origin)&&primaryWitnessFinite3(direction)
   &&primaryWitnessFinite(clip)&&clip>0.0&&clip<=1e20
   &&all(bitcast<vec4<u32>>(vec4<f32>(origin,0.0))==bitcast<vec4<u32>>(vec4<f32>(retainedOrigin,0.0)));
 retainedPrimary=retainedActive&&all(origin==retainedOrigin);
 let hit=primaryWitness_retainedOriginalNearest(origin,direction,-1,1e20);
 retainedPrimary=false;
 primaryWitnessHit=hit;
 // Analytic bars retain the original finite query: their cloned primary hit
 // changed one native output channel by one unit despite matching source arithmetic.
 primaryWitnessAvailable=primaryWitnessGood&&hit.panel>=0&&!(hit.panel>=720000&&hit.panel<720003)&&hit.distance>0.0&&hit.distance<clip
   &&primaryWitnessFinite(hit.distance)&&primaryWitnessFinite3(hit.normal);
 return hit;
}
"#;
