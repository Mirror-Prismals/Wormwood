//! Remove only the camera-visible floor effect. Secondary gem rays retain the
//! original floor, light visibility, reflection/refraction and engraving code.
fn once(s:&mut String,old:&str,new:&str)->Result<(),String>{
    if s.matches(old).count()!=1{return Err(format!("plain backdrop shader marker changed: {old}"));}
    *s=s.replacen(old,new,1);Ok(())
}
pub(super) fn source(original:&str)->Result<String,String>{
    let mut s=original.to_owned();
    once(&mut s,"        result+=gemTrace(origin,direction,random);","        result+=workstationCameraSample(origin,direction,random);")?;
    s.push_str(r#"
// No shadow field, floor BRDF or optical integration for the visible backdrop.
// Keep the authored ambient floor color and directly visible studio emitters.
fn workstationBackdrop(origin:vec3<f32>,direction:vec3<f32>)->vec3<f32> {
    let environment=gemEnvironment(origin,direction);
    if(environment.x<0.0){return gemRow(6).xyz*gemRow(5).xyz;}
    return environment.xyz;
}
fn workstationCameraSample(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {
    let environment=gemEnvironment(origin,direction);
    // A custom authored background keeps its original behavior.
    if(environment.x<0.0&&gemRow(245).z<0.5){
        let hit=gemNearest(origin,direction,-1,environment.w);
        if(hit.panel<0){return workstationBackdrop(origin,direction);}
    }
    return gemTrace(origin,direction,random);
}
"#);
    Ok(s)
}
pub(super) fn retained(s:&mut String)->Result<(),String>{
    if !s.contains("fn workstationBackdrop("){return Ok(());}
    // Group 0 retains only coverage/depth for empty backdrop pixels. Never bake
    // their lighting: no stale floor picture or floor-specific refresh remains.
    once(s,"value=vec4<f32>(gemTrace(origin,direction,random),1e19);","value=vec4<f32>(0.0,0.0,0.0,1e19);")?;
    once(s,"if(group==0||h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),select(h.distance,1e19,group==0));}","if(group==0){value=vec4<f32>(0.0,0.0,0.0,1e19);}\n  else if(h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),h.distance);}")?;
    let at="  let origin=retainedRay(p,sample);let direction=gemSafeUnit(gemRow(4).xyz);";
    once(s,at,&format!("{at}\n  if(best.w==1e19){{best=vec4<f32>(workstationBackdrop(origin,direction),best.w);}}"))?;
    Ok(())
}
#[cfg(test)] mod backdrop_tests {
    use super::*;
    const ORIGINAL:&str=include_str!("../Procedures/Shaders/workstation.wgsl");
    fn function<'a>(s:&'a str,name:&str)->&'a str{
        let start=s.find(&format!("fn {name}(" )).unwrap();let body=start+s[start..].find('{').unwrap();let mut depth=0;
        for (i,b) in s.as_bytes().iter().enumerate().skip(body){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return &s[start..=i];}}}panic!("unclosed function")
    }
    fn validate(s:&str){let m=naga::front::wgsl::parse_str(s).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();}
    #[test] fn backdrop_only_camera_misses_preserve_foreground_optical_functions(){
        let s=source(ORIGINAL).unwrap();validate(&s);
        for name in ["gemTrace","gemFloor","gemOpaqueSurface","gemShadow","gemTerminal","gemEnvironment","gemDisplay"]{assert_eq!(function(ORIGINAL,name),function(&s,name),"{name}");}
        let flat=function(&s,"workstationBackdrop");for forbidden in ["gemFloor(","gemTrace(","gemShadow(","gemCachedShadow("]{assert!(!flat.contains(forbidden));}
        assert!(function(&s,"workstationCameraSample").contains("if(hit.panel<0)"));
        assert!(function(&s,"workstationCameraSample").contains("gemRow(245).z<0.5"));
    }
    #[test] fn backdrop_rejects_changed_primary_entry_instead_of_silently_skipping(){
        assert!(source(&ORIGINAL.replace("result+=gemTrace(origin,direction,random);","result+=vec3<f32>(0.0);")).is_err());
    }
}
