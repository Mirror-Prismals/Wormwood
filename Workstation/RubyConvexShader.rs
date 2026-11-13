//! Exact, guarded specialization of intrinsic transport for panels 0..3.
//!
//! This is the Rust equivalent of the verified diagnostic transforms
//! ruby_convex_bake -> ruby_convex_escape -> ruby_convex_split. The dielectric
//! arithmetic and authored intersection routines are copied from the input,
//! not reimplemented or reassociated. A convex panel has one continuing
//! interior branch; exterior branches terminate. Reentry, multiple continuing
//! branches, an opaque material, or changed event/depth ordering rejects the
//! speculative sample and requests the original integrator in a second pass.
//!
//! The host must dispatch cs_ruby_response and cs_ruby_response_general as
//! separate passes using the same dirty plan, before resolve/playback. The
//! second pass resets all accumulators and replaces rejected sample rows.
//! Its queue and control intersection code are unreachable from the first
//! pipeline. Cache row29 is [attempted, accepted, fallback, attempted events].
//! No cache dimensions, bindings, sampling, optical budgets or artwork change.

fn once(source: &str, old: &str, new: &str) -> Result<String, String> {
    if old.is_empty() || source.matches(old).count() != 1 {
        return Err(format!("convex ruby shader marker changed: {}", old.lines().next().unwrap_or(old)));
    }
    Ok(source.replacen(old, new, 1))
}

fn closing(source: &str, opening: usize) -> Result<usize, String> {
    if source.as_bytes().get(opening) != Some(&b'{') {
        return Err("convex ruby shader block opening changed".into());
    }
    let mut depth = 1;
    let mut end = opening + 1;
    while depth > 0 && end < source.len() {
        match source.as_bytes()[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => (),
        }
        end += 1;
    }
    if depth != 0 { return Err("convex ruby shader block is unclosed".into()); }
    Ok(end)
}

// Return marker start, body start, body end, and one past the closing brace.
fn block(source: &str, marker: &str) -> Result<(usize, usize, usize, usize), String> {
    if source.matches(marker).count() != 1 {
        return Err(format!("convex ruby shader block changed: {marker}"));
    }
    let start = source.find(marker).ok_or("missing convex ruby block")?;
    let opening = start + source[start..].find('{').ok_or("missing convex ruby body")?;
    let end = closing(source, opening)?;
    Ok((start, opening + 1, end - 1, end))
}

fn function<'a>(source: &'a str, name: &str) -> Result<&'a str, String> {
    let (start, _, _, end) = block(source, &format!("fn {name}("))?;
    Ok(&source[start..end])
}

fn convex(source: &str) -> Result<String, String> {
    let original = function(source, "rubyBakeTrace")?;
    for name in ["gemEngraving(", "gemControlEngraving(",
                 "gemPluginNavigationEngraving(", "gemTimelineEngraving("] {
        if original.contains(name) {
            return Err(format!("convex ruby requires unengraved intrinsic bake: {name}"));
        }
    }
    for marker in ["for(var event:i32=0;event<64;event++)",
                   "if(f32(event)>=gemRow(8).w){break;}",
                   "var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);"] {
        if original.matches(marker).count() != 1 {
            return Err(format!("convex ruby event/init marker changed: {marker}"));
        }
    }
    let (_, begin, end, _) = block(original, "if(!finished&&!membrane){")?;
    let material = &original[begin..end];
    let (opaque_start, _, _, opaque_end) = block(material, "if(material.w<0.0001){")?;
    let prefix = &material[..opaque_start];
    let remaining = &material[opaque_end..];
    if !remaining.trim_start().starts_with("else{") {
        return Err("convex ruby dielectric branch changed".into());
    }
    // Parse only this leading else: nested dielectric branches also use else.
    let opening = remaining.find("else{").ok_or("missing ruby dielectric else")? + "else".len();
    let block_end = closing(remaining, opening)?;
    if !remaining[block_end..].trim().is_empty() {
        return Err("unexpected work after convex ruby dielectric branch".into());
    }
    let mut dielectric = remaining[opening + 1..block_end - 1].to_owned();
    for (weight, branch) in [("reflectWeight", "Reflection"), ("transmitWeight", "Transmission")] {
        let terminal = format!("if(terminal.w>0.5){{result+={weight}*terminal.xyz;retain{branch}=false;}}");
        dielectric = once(&dielectric, &terminal, &format!("{terminal}\n                else{{rubyConvexUnsupported=true;return vec3<f32>(0.0);}}"))?;
    }
    dielectric = once(&dielectric, OLD_CONTINUATION, NEW_CONTINUATION)?;
    if dielectric.matches("rubyBakeTerminal(").count() != 2 {
        return Err("convex ruby terminal call count changed".into());
    }
    dielectric = dielectric.replace("rubyBakeTerminal(", "rubyConvexTerminal(");
    if dielectric.contains("pending[") || dielectric.contains("count") {
        return Err("convex ruby dielectric still depends on the generic ray queue".into());
    }
    let kernel = format!("{HEAD}{prefix}if(material.w<0.0001){{rubyConvexUnsupported=true;return vec3<f32>(0.0);}}\n{dielectric}{TAIL}");
    let compute = function(source, "cs_ruby_response")?;
    let changed = once(compute, ORIGINAL_DISPATCH, DISPATCH)?;
    let changed = once(&changed,
        "        rubyResponse.rows[address+29u]=vec4<f32>(0.0);",
        "        rubyResponse.rows[address+29u]=rubyConvexStats;")?;
    Ok(once(source, compute, &changed)? + SUPPORT + &kernel)
}

fn escape(source: &str) -> Result<String, String> {
    // Save the actual authored plane selected by exactly the original strict
    // near/far comparisons. Do not derive a plane from a rounded hit point.
    let entry = function(source, "gemEnterPanel")?;
    let entry = once(entry, "fn gemEnterPanel(", "fn rubyConvexEnterPanel(")?;
    let entry = once(&entry, "var base:i32=gemPanelRow(panel);",
        "rubyConvexPlane=vec4<f32>(0.0);var base:i32=gemPanelRow(panel);")?;
    let entry = once(&entry, "near=t;entry=plane.xyz;",
        "near=t;entry=plane.xyz;rubyConvexPlane=plane;")?;
    let exit = function(source, "gemExitPanel")?;
    let exit = once(exit, "fn gemExitPanel(", "fn rubyConvexExitPanel(")?;
    let exit = once(&exit, "var base:i32=gemPanelRow(panel);",
        "rubyConvexPlane=vec4<f32>(0.0);var base:i32=gemPanelRow(panel);")?;
    let exit = once(&exit, "far=t;exitNormal=plane.xyz;",
        "far=t;exitNormal=plane.xyz;rubyConvexPlane=plane;")?;
    let kernel = function(source, "rubyConvexBakeTrace")?;
    let changed = once(kernel, "gemEnterPanel(", "rubyConvexEnterPanel(")?;
    let changed = once(&changed, "gemExitPanel(", "rubyConvexExitPanel(")?;
    let mut result = once(source, kernel, &changed)?;
    let terminal = function(&result, "rubyConvexTerminal")?;
    // One original clipping plane suffices to prove no positive entry: its
    // parallel test rejects, or its positive denominator makes far <= 0.
    // Additional planes only narrow that interval. Otherwise use the complete
    // original entry query. Its failure to prove escape rejects the sample.
    let certified = terminal.replacen('{', &format!("{{{CERTIFICATE}"), 1);
    result = once(&result, terminal, &certified)?;
    Ok(result + "\nvar<private> rubyConvexPlane:vec4<f32>;\n" + &entry + "\n" + &exit + "\n")
}

fn split(source: &str) -> Result<String, String> {
    let compute = function(source, "cs_ruby_response")?;
    let panel = once(compute, DISPATCH, PANEL_DISPATCH)?;
    let panel = once(&panel,
        "    if(rubyBakeTarget<0||rubyBakeTarget>=16644){return;}",
        "    if(rubyBakeTarget<0||rubyBakeTarget>=4){return;}")?;
    if panel.contains("rubyBakeTrace(") || panel.contains("rubyConvexResetBake(") {
        return Err("convex ruby panel entry still calls the general bake".into());
    }
    let general = once(compute, "fn cs_ruby_response(", "fn cs_ruby_response_general(")?;
    let general = once(&general, "    for(var sample:u32=0u;sample<2u;sample++){\n", GENERAL_SAMPLE)?;
    let general = once(&general, DISPATCH, GENERAL_DISPATCH)?;
    if general.contains("rubyConvexBakeTrace(") {
        return Err("convex ruby general entry repeats the speculative bake".into());
    }
    // Each invocation owns both samples of one texel. A separate pass, rather
    // than a barrier inside a workgroup, orders all row29 writes before reads.
    Ok(once(source, compute, &panel)? + "\n@compute @workgroup_size(8,8,1)\n" + &general + "\n")
}

/// Transform the completed, unengraved ruby-response shader before validation.
/// Callers must keep the canonical fallback on Err; never dispatch a partial
/// transformation. No renderer state is mutated while assembling this source.
pub(super) fn transform(source: &str) -> Result<String, String> {
    if source.len() > 4 * 1024 * 1024 {
        return Err("convex ruby shader exceeds source bound".into());
    }
    for name in ["rubyConvexBakeTrace", "rubyConvexTerminal", "rubyConvexEnterPanel",
                 "rubyConvexExitPanel", "rubyConvexResetBake", "cs_ruby_response_general"] {
        if source.contains(&format!("fn {name}(")) {
            return Err(format!("convex ruby shader already contains {name}"));
        }
    }
    split(&escape(&convex(source)?)?)
}

const ORIGINAL_DISPATCH: &str = "        let radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);";

const OLD_CONTINUATION: &str = r#"            if(retainReflection&&retainTransmission){
                if(reflectPriority>transmitPriority){
                    current=reflection;if(count<16){pending[count]=transmission;count++;}
                }else{
                    current=transmission;if(count<16){pending[count]=reflection;count++;}
                }
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"#;

const NEW_CONTINUATION: &str = r#"            if(retainReflection&&retainTransmission){
                rubyConvexUnsupported=true;return vec3<f32>(0.0);
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"#;

const SUPPORT: &str = r#"
var<private> rubyConvexStats:vec4<f32>;
var<private> rubyConvexUnsupported:bool;

// A rejected speculative kernel may already have emitted terminal leaves.
// Restore every accumulator used by the original bake before its exact rerun.
fn rubyConvexResetBake() {
    rubyBakeHead=vec4<f32>(0.0);
    rubyBakeDropped=vec3<f32>(0.0);
    rubyBakeLeaves=0;
    for(var lobe:i32=0;lobe<8;lobe++){
        rubyBakePositions[lobe]=vec4<f32>(0.0);
        rubyBakeDirections[lobe]=vec3<f32>(0.0);
        rubyBakeWeights[lobe]=vec3<f32>(0.0);
    }
}

// The original component terminal query, specialized only by known panel ID.
// Keep the full authored entry test when the escape certificate cannot prove
// rejection. No control BVH is reachable from this specialized panel kernel.
fn rubyConvexTerminal(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32)->vec4<f32> {
    let hit=gemEnterPanel(origin,direction,gemInverseRay(direction),rubyBakeTarget,1e20);
    if(hit.panel>=0&&hit.distance>0.0){return vec4<f32>(0.0);}
    rubyRecord(origin,direction,weight,depth);
    return vec4<f32>(0.0,0.0,0.0,1.0);
}
"#;

const HEAD: &str = r#"
// For a convex panel, every retained exterior ray terminates immediately.
// Only its refracted entry or subsequent internal reflection can continue.
// This preserves event==depth; a violation restarts with the general kernel.
fn rubyConvexBakeTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {
    var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);
    var result:vec3<f32>=vec3<f32>(0.0);
    for(var event:i32=0;event<64;event++){
        if(f32(event)>=gemRow(8).w){break;}
        rubyConvexStats.w+=1.0;
        if(current.depth!=event||(event>0&&current.medium!=rubyBakeTarget)){
            rubyConvexUnsupported=true;return vec3<f32>(0.0);
        }
        var hit:GemHit=GemHit(1e20,vec3<f32>(0.0),-1);
        if(current.medium>=0){hit=gemExitPanel(current.origin,current.direction,rubyBakeTarget);}
        else{hit=gemEnterPanel(current.origin,current.direction,gemInverseRay(current.direction),rubyBakeTarget,1e20);}
        if(event==0&&hit.panel>=0&&hit.distance>0.0){rubyBakeHead=vec4<f32>((current.origin+current.direction*hit.distance-rubyBakePose.xyz)/rubyBakePose.w,1.0);}
        var finished:bool=hit.panel<0||hit.distance<=0.0;
        if(finished){
            if(current.medium<0){rubyRecord(current.origin,current.direction,current.weight,current.depth);}
            break;
        }
        if(hit.panel!=rubyBakeTarget||hit.panel<0||hit.panel>=4){
            rubyConvexUnsupported=true;return vec3<f32>(0.0);
        }
"#;

const TAIL: &str = r#"
        if(finished){break;}
    }
    return result;
}
"#;

const DISPATCH: &str = r#"        rubyConvexStats=vec4<f32>(0.0);rubyConvexUnsupported=false;
        var radiance:vec3<f32>=vec3<f32>(0.0);
        if(rubyBakeTarget>=0&&rubyBakeTarget<4){
            rubyConvexStats.x=1.0;
            radiance=rubyConvexBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
            if(rubyConvexUnsupported){
                rubyConvexStats.z=1.0;
                rubyConvexResetBake();
                radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
            }else{rubyConvexStats.y=1.0;}
        }else{
            radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        }"#;

const CERTIFICATE: &str = r#"
    let denominator=dot(rubyConvexPlane.xyz,direction);
    let distance=rubyConvexPlane.w-dot(rubyConvexPlane.xyz,origin);
    let parallelReject=abs(denominator)<0.0000001&&distance< -0.0000001;
    let farReject=denominator>=0.0000001&&distance<=0.0;
    if(parallelReject||farReject){
        rubyRecord(origin,direction,weight,depth);
        return vec4<f32>(0.0,0.0,0.0,1.0);
    }
"#;

const PANEL_DISPATCH: &str = r#"        rubyConvexStats=vec4<f32>(1.0,0.0,0.0,0.0);
        rubyConvexUnsupported=false;
        let radiance=rubyConvexBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        if(rubyConvexUnsupported){rubyConvexStats.z=1.0;}
        else{rubyConvexStats.y=1.0;}"#;

const GENERAL_SAMPLE: &str = r#"    for(var sample:u32=0u;sample<2u;sample++){
        let generalAddress=first+sample*30u;
        var savedStats:vec4<f32>=vec4<f32>(0.0);
        if(rubyBakeTarget<4){
            savedStats=rubyResponse.rows[generalAddress+29u];
            if(savedStats.z<0.5){continue;}
        }
"#;

const GENERAL_DISPATCH: &str = r#"        let radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        rubyConvexStats=savedStats;"#;

#[cfg(test)]
mod tests {
    use super::*;

    // Self-contained source-generator fixture. The actual authored dielectric
    // arithmetic and entry/exit routines are used; only the surrounding bake
    // setup is a bounded fixture, so this module can also be #[path]-included
    // without depending on its eventual parent module or a graphics device.
    fn fixture() -> String {
        let canonical = include_str!("../Procedures/Shaders/workstation.wgsl");
        let trace = function(canonical, "gemTrace").unwrap();
        let (_, begin, end, _) = block(trace, "if(!finished&&!membrane){").unwrap();
        let material = &trace[begin..end];
        let (_, _, _, opaque_end) = block(material, "if(material.w<0.0001){").unwrap();
        let dielectric = material[opaque_end..].trim_start()
            .replace("gemTerminal(point+reflected*epsilon,reflected)",
                "rubyBakeTerminal(point+reflected*epsilon,reflected,reflectWeight,depth)")
            .replace("gemTerminal(point+transmitted*epsilon,transmitted)",
                "rubyBakeTerminal(point+transmitted*epsilon,transmitted,transmitWeight,depth)");
        let mut result = format!("{}\n{}\n", function(canonical, "gemEnterPanel").unwrap(), function(canonical, "gemExitPanel").unwrap());
        result.push_str("fn rubyBakeTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {\n");
        result.push_str("var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);\n");
        result.push_str("for(var event:i32=0;event<64;event++){\nif(f32(event)>=gemRow(8).w){break;}\n");
        result.push_str("if(!finished&&!membrane){\nlet prefixSentinel=17.25;\nif(material.w<0.0001){finished=true;}");
        result.push_str(&dielectric);
        result.push_str("\n}\n}\nreturn result;\n}\n");
        // Copy the real compute source constant verbatim. This avoids a second
        // handcrafted fixture that could omit a new accumulator/reset/store.
        let generator = include_str!("RubyResponseShader.rs");
        let marker = "const COMPUTE: &str = r#\"";
        let start = generator.find(marker).unwrap() + marker.len();
        let end = start + generator[start..].find("\"#;").unwrap();
        result.push_str(&generator[start..end]);
        result
    }

    #[test]
    fn convex_ruby_keeps_original_trace_and_exact_intersection_operations() {
        let source = fixture();
        let result = transform(&source).unwrap();
        assert_eq!(function(&result, "rubyBakeTrace").unwrap(), function(&source, "rubyBakeTrace").unwrap());
        for (original, copied, update) in [
            ("gemEnterPanel", "rubyConvexEnterPanel", "near=t;entry=plane.xyz;"),
            ("gemExitPanel", "rubyConvexExitPanel", "far=t;exitNormal=plane.xyz;"),
        ] {
            let restored = function(&result, copied).unwrap()
                .replace(copied, original)
                .replace("rubyConvexPlane=vec4<f32>(0.0);", "")
                .replace(&format!("{update}rubyConvexPlane=plane;"), update);
            assert_eq!(restored, function(&source, original).unwrap());
        }
        let kernel = function(&result, "rubyConvexBakeTrace").unwrap();
        assert!(kernel.contains("let prefixSentinel=17.25;"));
        for preserved in [
            "fract(random+vec2<f32>(f32(current.depth)*0.754877666,f32(event)*0.569840296))",
            "current.weight/=gemEnergy(hit.panel,inside,dot(-current.direction,n));",
            "var retainReflection:bool=canContinue&&reflectPriority>0.00005;",
            "var retainTransmission:bool=canContinue&&transmitPriority>0.00005;",
        ] { assert!(kernel.contains(preserved), "lost canonical arithmetic: {preserved}"); }
        assert!(!kernel.contains("pending["));
        assert!(!kernel.contains("rubyComponentHit("));
    }

    #[test]
    fn convex_ruby_separates_panel_pipeline_and_per_sample_fallback() {
        let result = transform(&fixture()).unwrap();
        let panel = function(&result, "cs_ruby_response").unwrap();
        let general = function(&result, "cs_ruby_response_general").unwrap();
        assert!(panel.contains("if(rubyBakeTarget<0||rubyBakeTarget>=4){return;}"));
        assert!(!panel.contains("rubyBakeTrace("));
        assert!(panel.contains("rubyConvexStats.z=1.0;"));
        assert!(!general.contains("rubyConvexBakeTrace("));
        let flag = general.find("if(savedStats.z<0.5){continue;}").unwrap();
        let reset = general.find("rubyBakeHead=vec4<f32>(0.0);").unwrap();
        let rerun = general.find(ORIGINAL_DISPATCH).unwrap();
        let store = general.find("rubyResponse.rows[address]=rubyBakeHead;").unwrap();
        assert!(flag < reset && reset < rerun && rerun < store);
        for source in [panel, general] {
            assert!(source.contains("for(var sample:u32=0u;sample<2u;sample++)"));
            assert!(source.contains("rubyResponse.rows[address+27u]=vec4<f32>(0.0);"));
            assert!(source.contains("rubyResponse.rows[address+28u]=vec4<f32>(0.0);"));
            assert!(source.contains("rubyResponse.rows[address+29u]=rubyConvexStats;"));
        }
        let terminal = function(&result, "rubyConvexTerminal").unwrap();
        assert!(terminal.find("if(parallelReject||farReject)").unwrap() < terminal.find("let hit=gemEnterPanel(").unwrap());
        assert!(terminal.contains("if(hit.panel>=0&&hit.distance>0.0){return vec4<f32>(0.0);}"));
    }

    #[test]
    fn convex_ruby_rejects_changed_contracts_without_partial_output() {
        let source = fixture();
        for (old, new) in [
            ("event<64", "event<128"),
            ("gemRow(8).w){break;}", "gemRow(8).z){break;}"),
            ("near=t;entry=plane.xyz;", "entry=plane.xyz;near=t;"),
            ("far=t;exitNormal=plane.xyz;", "exitNormal=plane.xyz;far=t;"),
            ("sample<2u", "sample<3u"),
            ("rubyResponse.rows[address+29u]=vec4<f32>(0.0);", ""),
            ("let prefixSentinel=17.25;", "gemEngraving();"),
        ] {
            assert!(source.contains(old));
            assert!(transform(&source.replace(old, new)).is_err(), "accepted changed marker: {old}");
        }
        assert!(transform(&transform(&source).unwrap()).is_err());
        assert!(transform("invalid source").is_err());
        assert!(once("marker marker", "marker", "changed").is_err());
        assert!(closing("{", 0).is_err());
    }
}
