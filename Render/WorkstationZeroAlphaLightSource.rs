//! Opt-in zero-contribution shortcut for track-I/O engraving only. Requires
//! the exact audited pure lighting/exit functions; native image validation is
//! still required because source equivalence does not constrain driver codegen.
const MARKER:&str="// ZERO_ALPHA_IO_LIGHT_V1";
const ACCUMULATE:&str="        result+=transmission*alpha*gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w);";
const CONTRACTS:[(&str,u64);9]=[
 ("gemRow",0x78c85be3220abcbd),("gemControlPoseRow",0xe41895397afa7b00),
 ("gemPanelRow",0xb67bdee25ec12a9c),("gemControlNormal",0xbb3c4dd8b28f1e46),
 ("gemSafeUnit",0xbdc19e3da465a4c0),("gemControlExit",0x431cf3171a9c8045),
 ("gemFresnel",0x46174c1f1853a8c0),("gemControlEngravingLight",0x42f03d7a3ebb6b09),
 ("gemTrackIoEngraving",0xe77909eef6c98111),
];
fn function(s:&str,name:&str)->Result<(usize,usize),String>{let marker=format!("fn {name}(");if s.matches(&marker).count()!=1{return Err(format!("zero-alpha requires one {name}"));}let a=s.find(&marker).unwrap();let open=a+s[a..].find('{').ok_or("missing function body")?;let mut depth=0usize;for(i,b)in s.as_bytes().iter().enumerate().skip(open){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok((a,i+1));}}}Err("unclosed zero-alpha function".into())}
// A source-version guard, not an authenticity/security hash. Ignore only line
// comments/whitespace; any arithmetic or control-flow change rejects this proof.
fn contract_hash(s:&str)->u64{s.lines().flat_map(|line|line.split("//").next().unwrap().bytes()).filter(|b|!b.is_ascii_whitespace()).fold(0xcbf29ce484222325u64,|h,b|(h^u64::from(b)).wrapping_mul(0x100000001b3))}
fn validate(s:&str)->Result<(),String>{let m=naga::front::wgsl::parse_str(s).map_err(|e|e.emit_to_string(s))?;naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("zero-alpha WGSL: {e:?}"))?;Ok(())}
pub(crate) fn transform(input:&str,enabled:bool)->Result<String,String>{
 if !enabled{return Ok(input.to_owned());}
 if input.contains(MARKER)||input.contains("zeroAlphaIoLight"){return Err("zero-alpha shortcut already installed/reserved names occupied".into());}
 for (name,expected) in CONTRACTS{let(a,b)=function(input,name)?;let actual=contract_hash(&input[a..b]);if actual!=expected{return Err(format!("zero-alpha proof source changed: {name} {actual:016x} != {expected:016x}"));}}
 let(a,b)=function(input,"gemTrackIoEngraving")?;let old=&input[a..b];
 if old.matches(ACCUMULATE).count()!=1{return Err("zero-alpha accumulation changed".into());}
 let guarded=format!("        if(!(zeroAlphaIoLightCoefficient(alpha,transmission,result)&&zeroAlphaIoLightFinite(pose.xyz+point*pose.w,medium,pose.w))){{\n{ACCUMULATE}\n        }}");
 let mut out=input.to_owned();out.replace_range(a..b,&old.replacen(ACCUMULATE,&guarded,1));out.push('\n');out.push_str(MARKER);out.push('\n');out.push_str(HELPERS);validate(&out)?;Ok(out)
}
const HELPERS:&str=r#"
fn zeroAlphaIoLightCoefficient(alpha:f32,transmission:f32,result:vec3<f32>)->bool {
    if(bitcast<u32>(alpha)!=0u||!(transmission>=0.0&&transmission<=1.0)){return false;}
    let bits=bitcast<vec3<u32>>(result);
    // +0 or strictly positive NORMAL finite values only. Preserve signed zero
    // and do not skip an addition that could flush an existing subnormal.
    return all((bits==vec3<u32>(0u))|((bits>=vec3<u32>(0x00800000u))&(bits<vec3<u32>(0x7f800000u))));
}
fn zeroAlphaIoLightFinite(point:vec3<f32>,medium:i32,scale:f32)->bool {
    if(medium<16644||medium>=32004||!(scale>=0.01&&scale<=16.0)
       ||!all(abs(point.xy)<=vec2<f32>(4.0))||!(abs(point.z)<=0.1)){return false;}
    let instance=(medium-4)/256;
    let pose=gemRow(gemControlPoseRow(instance));
    if(bitcast<u32>(pose.w)!=bitcast<u32>(scale)||!all(abs(pose.xyz)<=vec3<f32>(4.0))){return false;}
    let base=gemPanelRow(medium);let material=gemRow(base);let interior=gemRow(base+2);
    // First candidate admits only the original BVH exit, not a convex plane path.
    if(!(interior.z<=0.0&&abs(interior.z)<=4096.0)
       ||!(material.y>=1.0&&material.y<=4.0)){return false;}
    let sigma=gemRow(base+1).xyz;
    if(!all(sigma>=vec3<f32>(0.0))||!all(sigma<=vec3<f32>(4096.0))
       ||!all(abs(gemRow(5).xyz)<=vec3<f32>(4096.0))
       ||!all(abs(gemRow(214).xyz)<=vec3<f32>(16.0))||!(abs(gemRow(223).w)<=16.0)){return false;}
    for(var light:i32=0;light<3;light++){
        let at=16+5*light;let location=gemRow(at).xyz;
        // Separation in Z guarantees distance>0.39 without an extra length().
        if(!all(abs(location)<=vec3<f32>(16.0))||!(abs(location.z)>=0.5)
           ||!all(abs(gemRow(at+3).xyz)<=vec3<f32>(2.0))
           ||!(abs(gemRow(at+1).w)<=16.0)||!(abs(gemRow(at+2).w)<=16.0)
           ||!all(abs(gemRow(at+4).xyz)<=vec3<f32>(4096.0))){return false;}
    }
    return true;
}
"#;

#[cfg(test)]mod tests{
 use super::*;
 fn coefficient(alpha:f32,t:f32,r:[f32;3])->bool{alpha.to_bits()==0&&(0.0..=1.0).contains(&t)&&r.into_iter().all(|v|v.to_bits()==0||(0x00800000..0x7f800000).contains(&v.to_bits()))}
 #[test]fn off_identity_and_contract_trivia_and_rejections(){let raw="not WGSL\n \t";assert_eq!(transform(raw,false).unwrap(),raw);assert!(transform(raw,true).is_err());assert_eq!(contract_hash("a+b // comment\n"),contract_hash("a + b"));assert_ne!(contract_hash("a+b"),contract_hash("a-b"));}
 #[test]fn zero_signed_subnormal_nonfinite_and_transmission_boundaries(){assert!(coefficient(0.,1.,[0.,f32::MIN_POSITIVE,1.]));for a in [-0.,f32::from_bits(1),-1.,f32::INFINITY,f32::NAN]{assert!(!coefficient(a,1.,[0.;3]));}for t in [-f32::MIN_POSITIVE,1.0000001,f32::INFINITY,f32::NAN]{assert!(!coefficient(0.,t,[0.;3]));}for v in [-0.,f32::from_bits(1),f32::from_bits(0x007fffff),-1.,f32::INFINITY,f32::NAN]{assert!(!coefficient(0.,1.,[v;3]));}}
 #[test]fn admitted_zero_product_addition_preserves_ieee_bits_for_both_lighting_signs(){let mut n=0;for r in [0.,f32::MIN_POSITIVE,1e-20,0.1,1.,1e20,f32::MAX]{for t in [-0.,0.,f32::MIN_POSITIVE,0.5,1.]{for light in [-1e28,-1.,-0.,0.,f32::MIN_POSITIVE,1.,1e28]{assert!(coefficient(0.,t,[r;3]));let original=r+(t*0.)*light;assert_eq!(original.to_bits(),r.to_bits());n+=1;}}}assert_eq!(n,245);}
 #[test]fn finite_guard_helpers_naga_validate_without_optical_calls(){let stub="fn gemRow(i:i32)->vec4<f32>{return vec4<f32>(f32(i));}\nfn gemControlPoseRow(i:i32)->i32{return i;}\nfn gemPanelRow(i:i32)->i32{return i;}\n";validate(&format!("{stub}{HELPERS}\n@compute @workgroup_size(1)fn probe(){{let a=zeroAlphaIoLightCoefficient(0.0,1.0,vec3<f32>(0.0));let b=zeroAlphaIoLightFinite(vec3<f32>(0.0),16644,1.0);}} ")).unwrap();for forbidden in ["gemControlExit(","gemControlEngravingLight(","length(","exp(","sqrt("]{assert!(!HELPERS.lines().filter(|l|!l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n").contains(forbidden));}}
 #[test]fn worst_case_finite_products_have_large_f32_headroom(){
  // Intentionally loose bounds; no reliance on Fresnel being <=1. Every
  // original intermediate is bounded, not merely the final mathematical sum.
  let distance_lo=0.39f64;let distance_hi=36.;let scale_lo=0.01;
  let absorption_exponent=4096.*distance_hi/scale_lo;assert!(absorption_exponent<1.5e7);
  let amplitude_bound=4./1e-7;let fresnel_bound=amplitude_bound*amplitude_bound;
  let transmission_bound=1.+fresnel_bound;let area_bound=4.*16.*16.;
  let factor_bound=transmission_bound*13.*area_bound/(3.14159265359*distance_lo*distance_lo);
  let return_bound=(4096.+3.*8192.*factor_bound)*16.*16.;
  assert!(factor_bound<1e20&&return_bound<1e28&&return_bound<f32::MAX as f64);
 }
 #[test]fn ordered_finite_admission_boundaries_reject_exceptional_inputs(){let bounded=|v:f32,b:f32|v.abs()<=b;for b in [0.1,0.5,2.,4.,16.,4096.]{assert!(bounded(b,b)&&bounded(-b,b));assert!(!bounded(f32::from_bits(b.to_bits()+1),b));assert!(!bounded(f32::NAN,b)&&!bounded(f32::INFINITY,b));}let scale=|s:f32|s>=0.01&&s<=16.;assert!(scale(0.01)&&scale(16.));for s in [0.,-0.,f32::NAN,f32::INFINITY,f32::from_bits(0.01f32.to_bits()-1)]{assert!(!scale(s));}}
}
