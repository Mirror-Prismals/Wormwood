//! Primary capture only: omit aggregate tests for families whose every instance
//! is already rejected by the retained leaf-group filter. No BVH node pruning.
pub(crate) const ENV:&str="WORMWOOD_WORKSTATION_PRIMARY_GROUP_GATES";
const MARKER:&str="// PRIMARY_GROUP_GATES_V1";
const SHADER_CONTRACTS:&[(&str,u64)]=&[
 ("fn gemControlHit(",0x5274e83b41c06b1b),
 ("fn retainedControlGroup(",0xfd9af09a9cf83835),
 ("fn retainedOriginalNearest(",0x689864ea3ee995ab),
 ("fn gemNearest(",0xb712ebc68997396a),
];
const HOST_CONTRACTS:&[(usize,&str,u64)]=&[
 (0,"fn update(",0x4e8ef8e1929db22f),
 (0,"fn button_bounds(",0xbeafcf06343ecb92),
 (0,"fn stage_completed(",0xe54f4cefe3324c8a),
 (0,"fn encode_stage(",0x90d319e4c89a7dc4),
 (0,"fn encode(",0xcd98078f09504139),
 (1,"fn new(",0x27ff38d47e25d373),
 (1,"fn encode(",0x98eeadc5522bd047),
 (2,"fn append(&self, rows: &mut [[f32; 4]])",0x9b16ac34612594ee),
 (2,"fn build_instance_packet(",0x1814383b58340ecc),
 (2,"fn pack_instance_tree(",0x376f859c20d488f4),
 (2,"fn validated_instance_entries(",0xe5baa036cff23fa2),
];
fn span(s:&str,marker:&str)->Result<(usize,usize),String>{if s.matches(marker).count()!=1{return Err(format!("group-gate contract needs one {marker}"));}let a=s.find(marker).unwrap();let open=a+s[a..].find('{').ok_or("missing contract body")?;let mut d=0;for(i,b)in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{d+=1;}else if *b==b'}'{d-=1;if d==0{return Ok((a,i+1));}}}Err("unclosed group-gate contract".into())}
fn function<'a>(s:&'a str,marker:&str)->Result<&'a str,String>{let(a,b)=span(s,marker)?;Ok(&s[a..b])}
fn normalized(s:&str)->String{s.lines().flat_map(|line|line.split("//").next().unwrap().chars()).filter(|c|!c.is_whitespace()).collect()}
fn host_normalized(s:&str)->String{s.chars().filter(|c|!c.is_whitespace()).collect()}
fn host_hash(s:&str)->u64{host_normalized(s).bytes().fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(b)).wrapping_mul(0x100000001b3))}
fn hash(s:&str)->u64{normalized(s).bytes().fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(b)).wrapping_mul(0x100000001b3))}
fn once(s:&mut String,a:&str,b:&str)->Result<(),String>{if s.matches(a).count()!=1{return Err(format!("group-gate shader marker changed: {a}"));}*s=s.replacen(a,b,1);Ok(())}
fn host_contract(prerender:&str,incremental:&str,controls:&str)->Result<(),String>{
 let sources=[prerender,incremental,controls];
 for(i,marker,expected)in HOST_CONTRACTS{if host_hash(function(sources[*i],marker)?)!=*expected{return Err(format!("group-gate host contract changed: source{i} {marker}"));}}
 let host=host_normalized(prerender);let controls=host_normalized(controls);
 for marker in ["lettransport_layers=merge_static&&!depth_first;","source::source(original,merge_static,depth_first,compact)?","transport_layers,rows:[[0.;4];META_ROWS]"]{
  if host.matches(marker).count()!=1{return Err(format!("group-gate construction contract changed: {marker}"));}
 }
 if host.contains("self.transport_layers=")||host.contains("p.transport_layers="){return Err("group-gate transport ownership mutated".into());}
 if controls.matches("constMAX_INSTANCES:usize=128;").count()!=1{return Err("group-gate instance domain changed".into());}
 Ok(())
}
fn validated(s:&str)->Result<(naga::Module,naga::valid::ModuleInfo),String>{let m=naga::front::wgsl::parse_str(s).map_err(|e|e.emit_to_string(s))?;let info=naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("group-gate WGSL: {e:?}"))?;Ok((m,info))}
pub(crate) fn selected(shader:&str,prerender:&str,incremental:&str,controls:&str)->Result<String,String>{transform(shader,std::env::var(ENV).as_deref()==Ok("1"),prerender,incremental,controls)}
pub(crate) fn transform(shader:&str,enabled:bool,prerender:&str,incremental:&str,controls:&str)->Result<String,String>{
 if !enabled{return Ok(shader.to_owned());}
 if shader.contains("primaryGroupControlHit")||shader.contains("// PRIMARY_WITNESS_")||shader.contains("retainedOuterBegin(")||shader.contains("// METER_LEDGER_PLAYBACK_CERTIFICATE"){return Err("group gates duplicate/incompatible experiment source".into());}
 host_contract(prerender,incremental,controls)?;
 for(marker,expected)in SHADER_CONTRACTS{if hash(function(shader,marker)?)!=*expected{return Err(format!("group-gate shader contract changed: {marker}"));}}
 let(m,info)=validated(shader)?;
 let(h,_)=m.functions.iter().find(|(_,f)|f.name.as_deref()==Some("gemControlHit")).ok_or("missing original control query")?;
 for(g,v)in m.global_variables.iter(){if info[h][g].contains(naga::valid::GlobalUse::WRITE){return Err(format!("group-gate original query has side effects: {:?}",v.name));}}
 let mut clone=function(shader,"fn gemControlHit(")?.to_owned();
 once(&mut clone,"fn gemControlHit(","fn primaryGroupControlHit(")?;
 once(&mut clone,"    var groupInverse:vec3<f32>=gemInverseRay(ray);",PREFIX)?;
 for(a,b)in FAMILY_GATES{once(&mut clone,a,b)?;}
 let mut out=shader.to_owned();let(a,b)=span(&out,"fn retainedOriginalNearest(")?;let mut nearest=out[a..b].to_owned();
 once(&mut nearest,ORIGINAL_CALL,GATED_CALL)?;out.replace_range(a..b,&nearest);out.push('\n');out.push_str(MARKER);out.push('\n');out.push_str(&clone);out.push('\n');
 validated(&out)?;Ok(out)
}
const ORIGINAL_CALL:&str="    var control:GemHit=gemControlHit(origin,ray,-1,hit.distance,false);";
const GATED_CALL:&str=r#"    var control:GemHit=GemHit(hit.distance,vec3<f32>(0.0),-1);
    if(retainedPrimary){control=primaryGroupControlHit(origin,ray,-1,hit.distance,false);}
    else{control=gemControlHit(origin,ray,-1,hit.distance,false);}"#;
const PREFIX:&str=r#"    // Host-contract checked ownership; every unowned leaf already continues.
    let ownTop=retainedGroup==select(1,6,retainedTransportButtons);
    let ownBottom=retainedGroup==2;
    let ownMonitors=retainedGroup==5;
    let ownRight=retainedGroup==4;
    if(!ownTop&&!ownBottom&&!ownMonitors&&!ownRight){return hit;}
    var groupInverse:vec3<f32>=gemInverseRay(ray);"#;
const FAMILY_GATES:&[(&str,&str)]=&[
 ("var topVisible:bool=gemBox(","var topVisible:bool=ownTop&&gemBox("),
 ("var bottomVisible:bool=gemBox(","var bottomVisible:bool=ownBottom&&gemBox("),
 ("var monitorsVisible:bool=monitorCenter.w<monitorExtent.w&&gemBox(","var monitorsVisible:bool=ownMonitors&&monitorCenter.w<monitorExtent.w&&gemBox("),
 ("var ioVisible:bool=gemRow(44288).y>0.0&&gemBox(","var ioVisible:bool=ownRight&&gemRow(44288).y>0.0&&gemBox("),
 ("var keyboardVisible:bool=gemRow(44592).y>0.0&&gemBox(","var keyboardVisible:bool=ownRight&&gemRow(44592).y>0.0&&gemBox("),
];
