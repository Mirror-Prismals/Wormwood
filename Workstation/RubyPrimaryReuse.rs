//! Opt-in reuse of the exact primary query already performed by rubyFastTrace.
//! Payload is invocation-local; later fallback events keep their original queries.
use super::{function, replace_once};

pub(super) fn source(original: &str) -> Result<String, String> {
    if original.contains("struct RubyPrimaryResult") { return Err("primary reuse already installed".into()); }
    let mut output=original.to_owned();
    let (start,end)=function(&output,"rubyFastTrace")?;
    let mut fast=output[start..end].to_owned();
    replace_once(&mut fast,"random:vec2<f32>)->vec4<f32>","random:vec2<f32>)->RubyPrimaryResult")?;
    // Fractional budgets below one can still run fallback event zero. Mark this
    // early return unavailable, so that path executes its original query.
    let early="if(gemRow(8).w<1.0){return vec4<f32>(0.0,0.0,0.0,-1.0);}";
    replace_once(&mut fast,early,"if(gemRow(8).w<1.0){return RubyPrimaryResult(vec4<f32>(0.0,0.0,0.0,-1.0),vec4<f32>(0.0),GemHit(0.0,vec3<f32>(0.0),-1),false);}")?;
    let mut cursor=0;let mut returns=0;
    while let Some(offset)=fast[cursor..].find("return vec4<f32>(") {
        let at=cursor+offset;let end=at+fast[at..].find(';').ok_or("unterminated fast return")?;
        let expression=fast[at+7..end].to_owned();
        let replacement=format!("return RubyPrimaryResult({expression},environment,hit,true)");
        fast.replace_range(at..end,&replacement);cursor=at+replacement.len();returns+=1;
    }
    if returns!=5 { return Err(format!("primary fast return layout changed: {returns}")); }
    output.replace_range(start..end,&fast);
    let (start,end)=function(&output,"rubyTraceFallback")?;
    let mut fallback=output[start..end].to_owned();
    for required in ["var count:i32=0;","var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);",
                     "for(var event:i32=0;event<64;event++)","if(f32(event)>=gemRow(8).w){break;}"] {
        if fallback.matches(required).count()!=1 {return Err(format!("primary fallback precondition changed: {required}"));}
    }
    replace_once(&mut fallback,"random:vec2<f32>)->vec3<f32>","random:vec2<f32>,primary:RubyPrimaryResult)->vec3<f32>")?;
    replace_once(&mut fallback,ORIGINAL_QUERY,REUSED_QUERY)?;
    output.replace_range(start..end,&fallback);
    let (start,end)=function(&output,"gemTrace")?;
    let wrapper=output[start..end].replace("fastSample.w","fastSample.sample.w").replace("fastSample.xyz","fastSample.sample.xyz");
    let mut wrapper=wrapper;
    replace_once(&mut wrapper,"rubyTraceFallback(origin,direction,random)","rubyTraceFallback(origin,direction,random,fastSample)")?;
    output.replace_range(start..end,&wrapper);
    output.push_str("\nstruct RubyPrimaryResult { sample:vec4<f32>, environment:vec4<f32>, hit:GemHit, available:bool };\n");
    Ok(output)
}

const ORIGINAL_QUERY:&str=r#"        var environment:vec4<f32>=vec4<f32>(0.0,0.0,0.0,1e20);
        if(current.medium<0){environment=gemEnvironment(current.origin,current.direction);}
        var hit:GemHit=gemNearest(current.origin,current.direction,current.medium,environment.w);"#;
const REUSED_QUERY:&str=r#"        var environment:vec4<f32>=vec4<f32>(0.0,0.0,0.0,1e20);
        var hit:GemHit;
        if(event==0&&primary.available){
            environment=primary.environment;hit=primary.hit;
        }else{
            if(current.medium<0){environment=gemEnvironment(current.origin,current.direction);}
            hit=gemNearest(current.origin,current.direction,current.medium,environment.w);
        }"#;

#[cfg(test)] mod tests {
    use super::*;
    use naga::{Expression,Statement};
    fn generated()->String {super::super::source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap()}
    fn named<'a>(m:&'a naga::Module,name:&str)->&'a naga::Function {&m.functions.iter().find(|(_,f)|f.name.as_deref()==Some(name)).unwrap().1}
    fn query_calls(block:&naga::Block,m:&naga::Module)->Vec<String> {
        let mut out=Vec::new();
        for s in block {match s {
            Statement::Call{function,..}=>{let name=m.functions[*function].name.clone().unwrap();if name=="gemNearest"||name=="gemEnvironment"{out.push(name);}},
            Statement::Block(b)=>out.extend(query_calls(b,m)),
            Statement::If{accept,reject,..}=>{out.extend(query_calls(accept,m));out.extend(query_calls(reject,m));},
            Statement::Loop{body,continuing,..}=>{out.extend(query_calls(body,m));out.extend(query_calls(continuing,m));},
            Statement::Switch{cases,..}=>for c in cases{out.extend(query_calls(&c.body,m));},_=>()
        }}out
    }
    fn member(f:&naga::Function,h:naga::Handle<Expression>,argument:u32,index:u32)->bool {
        matches!(f.expressions[h],Expression::AccessIndex{base,index:i} if i==index&&matches!(f.expressions[base],Expression::FunctionArgument(a) if a==argument))
    }
    fn statements<'a>(block:&'a naga::Block,out:&mut Vec<&'a Statement>){
        for s in block {out.push(s);match s{
            Statement::Block(b)=>statements(b,out),
            Statement::If{accept,reject,..}=>{statements(accept,out);statements(reject,out);},
            Statement::Loop{body,continuing,..}=>{statements(body,out);statements(continuing,out);},
            Statement::Switch{cases,..}=>for c in cases{statements(&c.body,out);},_=>()
        }}
    }
    fn find_reuse<'a>(b:&'a naga::Block,f:&naga::Function)->Option<(&'a naga::Block,&'a naga::Block,naga::Handle<Expression>)> {
        for s in b {match s {
            Statement::If{condition,accept,reject}=>{
                if let Expression::Binary{op:naga::BinaryOperator::LogicalAnd,left,right}=f.expressions[*condition] {
                    if member(f,right,3,3){return Some((accept,reject,left));}
                }
                if let Some(x)=find_reuse(accept,f).or_else(||find_reuse(reject,f)){return Some(x);}
            },Statement::Loop{body,..}|Statement::Block(body)=>if let Some(x)=find_reuse(body,f){return Some(x);},_=>()
        }}None
    }
    #[test]fn generated_ir_reuses_only_first_available_query_and_keeps_later_queries(){
        let changed=source(&generated()).unwrap();super::super::validate(&changed).unwrap();
        let m=naga::front::wgsl::parse_str(&changed).unwrap();let fast=named(&m,"rubyFastTrace");
        assert_eq!(query_calls(&fast.body,&m),["gemEnvironment","gemNearest"]);
        let fallback=named(&m,"rubyTraceFallback");let (cached,uncached,first)=find_reuse(&fallback.body,fallback).unwrap();
        assert!(query_calls(cached,&m).is_empty());assert_eq!(query_calls(uncached,&m),["gemEnvironment","gemNearest"]);
        let Expression::Binary{op:naga::BinaryOperator::Equal,left,right}=fallback.expressions[first] else{panic!("not first event guard")};
        assert!(matches!(fallback.expressions[right],Expression::Literal(naga::Literal::I32(0))));
        let Expression::Load{pointer}=fallback.expressions[left]else{panic!("event not loaded")};
        let Expression::LocalVariable(event)=fallback.expressions[pointer]else{panic!("not local event")};
        assert_eq!(fallback.local_variables[event].name.as_deref(),Some("event"));
        let stores:Vec<_>=cached.iter().filter_map(|s|if let Statement::Store{pointer,value}=s{Some((*pointer,*value))}else{None}).collect();
        assert_eq!(stores.len(),2);
        for ((pointer,value),field) in stores.into_iter().zip([1,2]) {
            assert!(member(fallback,value,3,field));
            let Expression::LocalVariable(local)=fallback.expressions[pointer]else{panic!("not local destination")};
            assert_eq!(fallback.local_variables[local].name.as_deref(),Some(if field==1{"environment"}else{"hit"}));
        }
        let wrapper=named(&m,"gemTrace");assert!(query_calls(&wrapper.body,&m).is_empty());
        assert!(!m.global_variables.iter().any(|(_,v)|v.name.as_deref().is_some_and(|s|s.starts_with("rubyPrimary"))));
    }
    #[test]fn ir_payload_carries_the_same_environment_and_hit_to_the_only_fallback_caller(){
        let changed=source(&generated()).unwrap();let m=naga::front::wgsl::parse_str(&changed).unwrap();
        let fast=named(&m,"rubyFastTrace");let mut body=Vec::new();statements(&fast.body,&mut body);
        let query=|name:&str|body.iter().find_map(|s|if let Statement::Call{function,arguments,result:Some(result)}=s{
            (m.functions[*function].name.as_deref()==Some(name)).then_some((arguments,*result))
        }else{None}).unwrap();
        let (_,environment)=query("gemEnvironment");let (arguments,hit)=query("gemNearest");
        assert!(matches!(fast.expressions[arguments[3]],Expression::AccessIndex{base,index:3} if base==environment));
        let mut available=0;let mut unavailable=0;
        for statement in body {if let Statement::Return{value:Some(value)}=statement{
            let Expression::Compose{components,..}=&fast.expressions[*value]else{panic!("return lacks payload")};
            assert_eq!(components.len(),4);
            match fast.expressions[components[3]] {
                Expression::Literal(naga::Literal::Bool(true))=>{assert_eq!(components[1],environment);assert_eq!(components[2],hit);available+=1;},
                Expression::Literal(naga::Literal::Bool(false))=>unavailable+=1,
                _=>panic!("availability must be explicit"),
            }
        }}
        assert_eq!((available,unavailable),(5,1));
        let wrapper=named(&m,"gemTrace");let mut body=Vec::new();statements(&wrapper.body,&mut body);
        let fast_result=body.iter().find_map(|s|if let Statement::Call{function,result:Some(result),..}=s{
            (m.functions[*function].name.as_deref()==Some("rubyFastTrace")).then_some(*result)
        }else{None}).unwrap();
        let fallback_calls:Vec<_>=body.iter().filter_map(|s|if let Statement::Call{function,arguments,..}=s{
            (m.functions[*function].name.as_deref()==Some("rubyTraceFallback")).then_some(arguments)
        }else{None}).collect();assert_eq!(fallback_calls.len(),1);assert_eq!(fallback_calls[0][3],fast_result);
        let mut callers=0;
        for (_,f) in m.functions.iter(){let mut body=Vec::new();statements(&f.body,&mut body);for s in body{
            if let Statement::Call{function,..}=s{if m.functions[*function].name.as_deref()==Some("rubyTraceFallback"){callers+=1;assert_eq!(f.name.as_deref(),Some("gemTrace"));}}
        }}assert_eq!(callers,1);
    }
    #[test]fn no_other_shader_function_or_fallback_event_arithmetic_changes(){
        let before=generated();let after=source(&before).unwrap();
        let m=naga::front::wgsl::parse_str(&before).unwrap();
        for (_,f) in m.functions.iter(){let name=f.name.as_ref().unwrap();if ["rubyFastTrace","rubyTraceFallback","gemTrace"].contains(&name.as_str()){continue;}
            let (a,b)=function(&before,name).unwrap();let(c,d)=function(&after,name).unwrap();assert_eq!(&before[a..b],&after[c..d],"{name}");
        }
        let(a,b)=function(&before,"rubyTraceFallback").unwrap();let(c,d)=function(&after,"rubyTraceFallback").unwrap();
        let restored=after[c..d].replace("random:vec2<f32>,primary:RubyPrimaryResult)","random:vec2<f32>)").replace(REUSED_QUERY,ORIGINAL_QUERY);
        assert_eq!(&before[a..b],restored);
    }
    #[test]fn unavailable_early_budget_and_pixel_transform_both_validate(){
        let before=generated();let changed=source(&before).unwrap();
        assert!(changed.contains("GemHit(0.0,vec3<f32>(0.0),-1),false)"));
        let pixel=super::super::pixel::source(&before).unwrap();let pixel=source(&pixel).unwrap();super::super::validate(&pixel).unwrap();
        let original=include_str!("../Procedures/Shaders/workstation.wgsl");assert!(original.contains("if(f32(event)>=gemRow(8).w){break;}"));
        assert!(source(&changed).is_err());assert!(source(&before.replace(ORIGINAL_QUERY,"changed_query")).is_err());
    }
}
