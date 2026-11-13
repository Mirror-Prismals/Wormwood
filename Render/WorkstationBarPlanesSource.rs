//! Optional exact-value reuse of the original GPU bar-plane construction.
//! The host owns ordering: prepare this tail before any consumer/snapshot,
//! and upload only CPU_META_ROWS ordinary metadata rows. Missing, mismatched
//! or nonfinite table values retain the unchanged canonical calculation.
pub(crate) const CPU_META_ROWS: usize = 64;
pub(crate) const TABLE_ROWS: usize = 81;
pub(crate) const TOTAL_ROWS: usize = CPU_META_ROWS + TABLE_ROWS;
pub(crate) const COMPUTE_WORKGROUPS: u32 = 3;
const ROWS_PER_BAR: usize = 27;
const TAG: u32 = 11_619_537;
const MARKER: &str = "// RETAINED_BAR_PLANE_TABLE_V1";
const META: &str = "@group(0) @binding(6) var<storage,read> retainedMeta:RetainedMetadata;";
const CALL: &str = "gemBarPlane(j,size.xy)";

fn function<'a>(source: &'a str, name: &str) -> Result<&'a str, String> {
    let marker = format!("fn {name}(");
    if source.matches(&marker).count() != 1 {
        return Err(format!("bar plane source requires exactly one {name}"));
    }
    let start = source.find(&marker).unwrap();
    let open = start + source[start..].find('{').ok_or("bar plane function body missing")?;
    let mut depth = 0usize;
    for (i, byte) in source.as_bytes().iter().enumerate().skip(open) {
        if *byte == b'{' { depth += 1; }
        else if *byte == b'}' {
            depth -= 1;
            if depth == 0 { return Ok(&source[start..=i]); }
        }
    }
    Err("bar plane function body is unclosed".into())
}

fn constants() -> String {
    format!("{MARKER}\nconst retainedBarCpuRows:u32={CPU_META_ROWS}u;\nconst retainedBarTotalRows:u32={TOTAL_ROWS}u;\nconst retainedBarStride:u32={ROWS_PER_BAR}u;\nconst retainedBarTag:f32={TAG}.0;\n")
}

/// Apply after retained metadata is declared. The original plane function and
/// all intersection expressions/order remain unchanged apart from one call.
pub(crate) fn transform(source: &str) -> Result<String, String> {
    if source.contains(MARKER) || source.contains("fn retainedBarPlane(") {
        return Err("retained bar plane table already installed".into());
    }
    if source.matches(META).count() != 1 {
        return Err("retained bar plane table needs the original read-only metadata binding6".into());
    }
    let original_plane = function(source, "gemBarPlane")?;
    let hit = function(source, "gemBarHit")?;
    if source.matches(CALL).count() != 1 || hit.matches(CALL).count() != 1 {
        return Err("bar plane intersection call changed or duplicated".into());
    }
    for marker in ["for(var i:i32=0;i<3;i++)", "for(var j:i32=0;j<26;j++)",
                   "var size:vec4<f32>=gemRow(base+3+i);"] {
        if hit.matches(marker).count() != 1 {
            return Err(format!("bar plane intersection contract changed: {marker}"));
        }
    }
    let mut out = source.replacen(CALL, "retainedBarPlane(j,size.xy,i)", 1);
    out.push('\n'); out.push_str(&constants()); out.push_str(LOOKUP);
    if function(&out, "gemBarPlane")? != original_plane {
        return Err("original bar plane function was modified".into());
    }
    Ok(out)
}

const LOOKUP: &str = r#"
fn retainedBarPlane(index:i32,halfSize:vec2<f32>,instance:i32)->vec4<f32> {
    // Positive bounded finite dimensions avoid exceptional precomputation.
    // Larger/nonfinite/unsupported dimensions use the original function.
    if(index>=0&&index<26&&instance>=0&&instance<3
       &&all(halfSize>vec2<f32>(0.0))&&all(halfSize<=vec2<f32>(1000000.0))
       &&arrayLength(&retainedMeta.rows)>=retainedBarTotalRows){
        let at=retainedBarCpuRows+u32(instance)*retainedBarStride;
        let header=retainedMeta.rows[at];
        if(header.z==retainedBarTag&&header.w==1.0
           &&all(bitcast<vec2<u32>>(header.xy)==bitcast<vec2<u32>>(halfSize))){
            let plane=retainedMeta.rows[at+1u+u32(index)];
            // Ordered comparisons reject NaN and both infinities.
            if(all(abs(plane)<=vec4<f32>(3.4028234663852886e38))){return plane;}
        }
    }
    return gemBarPlane(index,halfSize);
}
"#;

/// A minimal independent shader; dispatch (3,1,1) groups of27 invocations.
/// Binding0 is the frozen/current source packet, binding1 the corresponding
/// enlarged metadata buffer. Each invocation writes exactly one tail row.
pub(crate) fn compute_source(original: &str) -> Result<String, String> {
    let plane = function(original, "gemBarPlane")?;
    let mut out = constants();
    out.push_str(COMPUTE_BINDINGS);
    out.push_str(plane); out.push('\n'); out.push_str(COMPUTE);
    // A new dependency in the extracted function must fail closed, rather
    // than accidentally bind live scene data or approximate it on the CPU.
    let module = naga::front::wgsl::parse_str(&out)
        .map_err(|e| format!("bar plane precompute WGSL: {}", e.emit_to_string(&out)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module).map_err(|e| format!("bar plane precompute validation: {e:?}"))?;
    Ok(out)
}

const COMPUTE_BINDINGS: &str = r#"
struct RetainedBarRows { rows:array<vec4<f32>> };
@group(0) @binding(0) var<storage,read> retainedBarFrame:RetainedBarRows;
@group(0) @binding(1) var<storage,read_write> retainedBarTable:RetainedBarRows;
"#;

const COMPUTE: &str = r#"
@compute @workgroup_size(27,1,1)
fn cs_retained_bar_planes(@builtin(global_invocation_id) id:vec3<u32>) {
    if(id.x>=81u||id.y!=0u||id.z!=0u
       ||arrayLength(&retainedBarTable.rows)<retainedBarTotalRows){return;}
    let instance=id.x/retainedBarStride;
    let part=id.x%retainedBarStride;
    let at=retainedBarCpuRows+instance*retainedBarStride+part;
    // Clear every row on a short packet; old headers cannot authorize reuse.
    if(arrayLength(&retainedBarFrame.rows)<=45749u){
        retainedBarTable.rows[at]=vec4<f32>(0.0);return;
    }
    let halfSize=retainedBarFrame.rows[45747u+instance].xy;
    if(!all(halfSize>vec2<f32>(0.0))||!all(halfSize<=vec2<f32>(1000000.0))){
        retainedBarTable.rows[at]=vec4<f32>(0.0);return;
    }
    if(part==0u){
        retainedBarTable.rows[at]=vec4<f32>(halfSize,retainedBarTag,1.0);
    }else{
        retainedBarTable.rows[at]=gemBarPlane(i32(part)-1,halfSize);
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    // Standalone fixture: no repository-relative include or external asset.
    // The original source function is included verbatim so both generated
    // modules can validate using only this file and the existing Naga crate.
    const PLANE: &str = r#"fn gemBarPlane(index:i32,halfSize:vec2<f32>)->vec4<f32> {
    var p:vec4<f32>=vec4<f32>(0.0,0.0,1.0,0.30);
    if(index==1){p=vec4<f32>(0.0,0.0,-1.0,0.85);}
    if(index>=2){
        var ring:i32=(index-2)/8;var side:i32=(index-2)%8;
        var angle:f32=f32(side)*0.7853981633974483;if(ring==2){angle+=0.39269908169872414;}
        var x:f32=cos(angle);var y:f32=sin(angle);
        var extent:f32=abs(x)*halfSize.x+abs(y)*halfSize.y-0.18*min(abs(x),abs(y));
        var slope:f32=extent*0.86/(0.85-0.07);var z:f32=0.0;var distance:f32=extent;
        if(ring==1){z=0.31/0.30;}
        if(ring==2){z=-slope;distance=extent+slope*0.07;}
        p=vec4<f32>(x,y,z,distance);
    }
    var magnitude:f32=length(p.xyz);return vec4<f32>(p.xyz/magnitude,(p.w-0.30*p.z)/magnitude);
}"#;
    fn fixture() -> String {
        format!(r#"{PLANE}
struct RetainedMetadata {{ rows:array<vec4<f32>> }};
{META}
@group(0) @binding(0) var<storage,read> scene:RetainedMetadata;
fn gemRow(index:i32)->vec4<f32>{{return scene.rows[u32(index)];}}
fn gemBarHit()->vec4<f32>{{
 var base:i32=45744;var result:vec4<f32>=vec4<f32>(0.0);
 for(var i:i32=0;i<3;i++){{
  var size:vec4<f32>=gemRow(base+3+i);
  for(var j:i32=0;j<26;j++){{result+=gemBarPlane(j,size.xy);}}
 }}return result;
}}
@compute @workgroup_size(1) fn fixture_entry(){{let value=gemBarHit();}}
"#)
    }
    fn validate(s: &str) -> (naga::Module, naga::valid::ModuleInfo) {
        let m = naga::front::wgsl::parse_str(s).unwrap_or_else(|e| panic!("{}",e.emit_to_string(s)));
        let i = naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();
        (m,i)
    }
    #[test]
    fn only_original_call_changes_and_plane_body_is_exact() {
        let before=fixture();let after=transform(&before).unwrap();validate(&before);validate(&after);
        assert_eq!(function(&after,"gemBarPlane").unwrap(),PLANE);
        let prefix=after.split(MARKER).next().unwrap().trim_end();
        assert_eq!(prefix.replace("retainedBarPlane(j,size.xy,i)",CALL),before.trim_end());
        assert!(transform(&after).is_err());
    }
    #[test]
    fn minimal_compute_has_only_two_audited_bindings_and81_invocations() {
        let s=compute_source(&fixture()).unwrap();let(m,info)=validate(&s);
        assert_eq!(function(&s,"gemBarPlane").unwrap(),PLANE);
        assert_eq!(m.entry_points.len(),1);assert_eq!(m.entry_points[0].workgroup_size,[27,1,1]);
        assert_eq!(COMPUTE_WORKGROUPS*27,TABLE_ROWS as u32);
        let usage=info.get_entry_point(0);
        let bindings:Vec<_>=m.global_variables.iter().filter_map(|(h,g)|
            (!usage[h].is_empty()).then(||g.binding.as_ref().unwrap().binding)).collect();
        assert_eq!(bindings,vec![0,1]);
        assert!(!s.contains("gemRow("));assert!(!s.contains("retainedMeta.rows"));
    }
    #[test]
    fn table_addresses_are_unique_and_never_touch_cpu_metadata() {
        let addresses:Vec<_>=(0..TABLE_ROWS).map(|id|CPU_META_ROWS+(id/27)*27+id%27).collect();
        assert_eq!(addresses,(CPU_META_ROWS..TOTAL_ROWS).collect::<Vec<_>>());
        assert_eq!([CPU_META_ROWS,CPU_META_ROWS+27,CPU_META_ROWS+54],[64,91,118]);
        assert_eq!(addresses.last(),Some(&144));
        assert_eq!(TOTAL_ROWS*16,2320);
    }
    #[test]
    fn lookup_guards_cover_missing_mismatched_and_nonfinite_values() {
        fn lookup_allowed(size:[f32;2],header:[f32;4],plane:[f32;4],rows:usize,i:i32,j:i32)->bool {
            (0..3).contains(&i)&&(0..26).contains(&j)&&rows>=TOTAL_ROWS
                &&size.iter().all(|v|*v>0.&&*v<=1e6)
                &&header[2]==TAG as f32&&header[3]==1.
                &&size.map(f32::to_bits)==[header[0],header[1]].map(f32::to_bits)
                &&plane.iter().all(|v|v.abs()<=f32::MAX)
        }
        let size=[37.03846,1.];let h=[size[0],size[1],TAG as f32,1.];let p=[0.,0.,1.,0.];
        assert!(lookup_allowed(size,h,p,TOTAL_ROWS,2,25));
        for n in [0,64,144]{assert!(!lookup_allowed(size,h,p,n,0,0));}
        for (i,j) in [(-1,0),(3,0),(0,-1),(0,26)]{assert!(!lookup_allowed(size,h,p,145,i,j));}
        let mut changed=h;changed[0]=f32::from_bits(h[0].to_bits()+1);assert!(!lookup_allowed(size,changed,p,145,0,0));
        for value in [0.,-0.,-1.,f32::NAN,f32::INFINITY,f32::NEG_INFINITY,1000001.] {
            assert!(!lookup_allowed([value,1.],[value,1.,TAG as f32,1.],p,145,0,0));
        }
        for value in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY] {
            let mut bad=p;bad[0]=value;assert!(!lookup_allowed(size,h,bad,145,0,0));
        }
        for lane in [2,3]{let mut bad=h;bad[lane]=0.;assert!(!lookup_allowed(size,bad,p,145,0,0));}
        let s=transform(&fixture()).unwrap();
        for required in ["bitcast<vec2<u32>>(header.xy)","arrayLength(&retainedMeta.rows)>=retainedBarTotalRows",
            "header.z==retainedBarTag&&header.w==1.0","return gemBarPlane(index,halfSize);",
            "all(abs(plane)<=vec4<f32>(3.4028234663852886e38))"] {assert!(s.contains(required));}
    }
    #[test]
    fn malformed_source_and_unexpected_dependencies_fail_closed() {
        assert!(transform("").is_err());assert!(compute_source("").is_err());
        let s=fixture();assert!(transform(&s.replace(META,"")).is_err());
        assert!(transform(&s.replace(CALL,"gemBarPlane(j,vec2<f32>(1.0))")).is_err());
        assert!(transform(&(s.clone()+"\nfn duplicate(){let p=gemBarPlane(j,size.xy);}")).is_err());
        assert!(compute_source(&(PLANE.to_string()+PLANE)).is_err());
        assert!(compute_source("fn gemBarPlane(index:i32,halfSize:vec2<f32>)->vec4<f32>{").is_err());
        assert!(compute_source(&PLANE.replace("var p:vec4<f32>=vec4<f32>(0.0,0.0,1.0,0.30);","var p:vec4<f32>=gemRow(1);")).is_err());
        assert!(compute_source(&PLANE.replace("return vec4<f32>(p.xyz/magnitude", "return invalid_type(p.xyz/magnitude")).is_err());
    }
}
