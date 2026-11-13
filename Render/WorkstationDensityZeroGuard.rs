//! Opt-in exact zero-density certificate. Original UV/LOD expressions run first;
//! all uncertified inputs retain the original two mip samples and interpolation.
//! Power-of-two mip addressing uses the exact bounded integer geometric sum.
//! Preparation only: native image equivalence and performance are unverified.
pub(crate) const ENV: &str = "WORMWOOD_WORKSTATION_DENSITY_ZERO_GUARD";
const MARKER: &str = "// DENSITY_ZERO_CERTIFICATE_V1";
const INSERT_AFTER: &str = "    var lod:f32=clamp(log2(max(footprint*gemRow(224).y/gemRow(224).z,1.0)),0.0,7.0);";
pub(crate) const GUARD: &str = r#"
    // DENSITY_ZERO_CERTIFICATE_V1
    // At every mip0..7 edge>=1. Far-outside UVs make all four bilinear
    // taps miss. Bounds also preserve finite fract and safe original i32 casts.
    let densityGuardEdge=gemRow(224).y;
    if(densityGuardEdge>=128.0&&densityGuardEdge<=16384.0&&floor(densityGuardEdge)==densityGuardEdge
       &&lod>=0.0&&lod<=7.0&&all(abs(uv)<=vec2<f32>(32768.0))
       &&(any(uv<vec2<f32>(-1.0))||any(uv>vec2<f32>(2.0)))){return 0.0;}
"#;
pub(crate) const MIP_LOOP: &str = r#"    for(var mip:i32=0;mip<7;mip++){
        if(mip>=level){break;}
        offset+=edge*edge;edge/=2;
    }"#;
pub(crate) const MIP_CERTIFICATE: &str = r#"    // DENSITY_MIP_INTEGER_CERTIFICATE_V1
    // The original cast above remains authoritative. Only integral power-of-two
    // headers and original levels0..7 use this exact geometric-series identity.
    if(edge>=128&&edge<=16384&&(edge&(edge-1))==0&&level>=0&&level<=7
       &&gemRow(224).y==f32(edge)){
        let densityBaseEdge=edge;
        edge=densityBaseEdge>>u32(level);
        offset=((densityBaseEdge*densityBaseEdge-edge*edge)*4)/3;
    }else{
        for(var mip:i32=0;mip<7;mip++){
            if(mip>=level){break;}
            offset+=edge*edge;edge/=2;
        }
    }"#;

const TEXEL: &str = r#"fn gemDensityTexel(glyph:i32,x:i32,y:i32,edge:i32,offset:i32,layer:i32)->f32 {
    if(x<0||x>=edge||y<0||y>=edge){return 0.0;}
    var index:i32=glyph*i32(gemRow(224).w)+offset+y*edge+x;
    var packed:vec4<f32>=gemRow(i32(gemRow(224).x)+index/2);
    var rg:f32=packed.x;var b:f32=packed.y;
    if(index%2!=0){rg=packed.z;b=packed.w;}
    if(layer==2){return b/255.0;}
    if(layer==1){return floor(rg/256.0)/255.0;}
    return f32(i32(rg)%256)/255.0;
}"#;
const LEVEL: &str = r#"fn gemDensityLevel(uv:vec2<f32>,glyph:i32,level:i32,layer:i32)->f32 {
    var edge:i32=i32(gemRow(224).y);var offset:i32=0;
    for(var mip:i32=0;mip<7;mip++){
        if(mip>=level){break;}
        offset+=edge*edge;edge/=2;
    }
    var p:vec2<f32>=uv*f32(edge)-vec2<f32>(0.5);var f:vec2<f32>=fract(p);
    var x:i32=i32(floor(p.x));var y:i32=i32(floor(p.y));
    var a:f32=gemDensityTexel(glyph,x,y,edge,offset,layer);
    var b:f32=gemDensityTexel(glyph,x+1,y,edge,offset,layer);
    var c:f32=gemDensityTexel(glyph,x,y+1,edge,offset,layer);
    var d:f32=gemDensityTexel(glyph,x+1,y+1,edge,offset,layer);
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}"#;
const DENSITY: &str = r#"fn gemEngravedDensity(em:vec2<f32>,glyph:i32,shift:vec2<f32>,footprint:f32,layer:i32)->f32 {
    var grid:vec4<f32>=gemRow(213);
    var uv:vec2<f32>=(em-grid.yz-shift*grid.x)/gemRow(224).z;
    var lod:f32=clamp(log2(max(footprint*gemRow(224).y/gemRow(224).z,1.0)),0.0,7.0);
    var lo:i32=i32(floor(lod));var hi:i32=min(lo+1,7);
    return mix(gemDensityLevel(uv,glyph,lo,layer),gemDensityLevel(uv,glyph,hi,layer),fract(lod));
}"#;
fn function<'a>(s:&'a str,name:&str)->Result<&'a str,String>{
    let marker=format!("fn {name}(");
    if s.matches(&marker).count()!=1{return Err(format!("density certificate requires one {name}"));}
    let begin=s.find(&marker).unwrap();let open=begin+s[begin..].find('{').ok_or("density function has no body")?;
    let mut depth=0usize;
    for(i,b)in s.as_bytes().iter().enumerate().skip(open){
        if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{return Ok(&s[begin..=i]);}}
    }
    Err("density function body is unclosed".into())
}
fn normalized(s:&str)->String{s.chars().filter(|c|!c.is_whitespace()).collect()}
pub(crate) fn selected(input:&str)->Result<String,String>{
    if std::env::var(ENV).as_deref()==Ok("1"){source(input)}else{Ok(input.to_owned())}
}
pub(crate) fn source(input:&str)->Result<String,String>{
    if input.contains(MARKER)||input.contains("densityGuardEdge")||input.contains("densityBaseEdge"){return Err("density certificate already present or reserved name used".into());}
    for(name,expected)in[("gemDensityTexel",TEXEL),("gemDensityLevel",LEVEL),("gemEngravedDensity",DENSITY)]{
        if normalized(function(input,name)?)!=normalized(expected){return Err(format!("density certificate assumptions changed in {name}"));}
    }
    if input.matches(INSERT_AFTER).count()!=1{return Err("density certificate LOD marker changed".into());}
    let mut output=input.replacen(INSERT_AFTER,&format!("{INSERT_AFTER}{GUARD}"),1);
    // Restrict replacement to the proved font-density function. Other samplers
    // may contain superficially similar loops with different address contracts.
    let original_level=function(&output,"gemDensityLevel")?.to_owned();
    if original_level.matches(MIP_LOOP).count()!=1{return Err("density mip loop marker changed".into());}
    let next_level=original_level.replacen(MIP_LOOP,MIP_CERTIFICATE,1);
    output=output.replacen(&original_level,&next_level,1);
    let module=naga::front::wgsl::parse_str(&output).map_err(|e|e.emit_to_string(&output))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all())
        .validate(&module).map_err(|e|format!("density certificate validation: {e:?}"))?;
    Ok(output)
}
