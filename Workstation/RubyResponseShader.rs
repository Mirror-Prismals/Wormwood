//! GPU-resident, shape-keyed optical response for the workstation's ruby parts.
//! The control thread owns invalidation. Separate compute kernels refresh dirty
//! convex panels and general/fallback entries; a subsequent pass resolves current studio lighting for
//! every active entry before rendering. Primary rays reuse transport; exterior rays
//! see the studio environment and floor directly, matching the approved preview.
//! Primary visibility and component self-intersections remain geometric. Missing
//! or unresolved cache entries use the integrator with that exterior policy.
//! Engraving uses the actual drawable sample's first refracted segment, including
//! its original density, absorption, lighting and opacity. Further internally
//! reflected engraving is omitted in this explicit appearance approximation.
//! The canonical shader remains unchanged as a separate pipeline fallback.

#[path = "RubyConvexShader.rs"]
mod convex;
#[path = "RubyPixelShader.rs"]
mod pixel;
#[path = "RubyPrimaryReuse.rs"]
mod primary_reuse;

fn replace_once(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("ruby response shader marker changed: {old}"));
    }
    *source = source.replacen(old, new, 1);
    Ok(())
}

fn block_end(source: &str, opening: usize) -> Result<usize, String> {
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
    if depth != 0 { return Err("ruby response shader body is unclosed".into()); }
    Ok(end)
}

fn function(source: &str, name: &str) -> Result<(usize, usize), String> {
    let marker = format!("fn {name}(");
    if source.matches(&marker).count() != 1 {
        return Err(format!("ruby response shader function changed: {name}"));
    }
    let start = source.find(&marker).ok_or("missing ruby response function")?;
    let opening = start + source[start..].find('{').ok_or("missing ruby response body")?;
    Ok((start, block_end(source, opening)?))
}

pub(crate) fn canonical_trace(original: &str) -> Result<String, String> {
    let (start, end) = function(original, "gemTrace")?;
    let mut trace = original[start..end].to_owned();
    // Accept the old four-cap experiment while its generated frontends are being
    // retired. It must never run alongside the material-wide primary response.
    if let Some(start) = trace.find("        var responseUsed:bool=false;") {
        let end = trace[start..].find("        if(!responseUsed&&finished&&current.medium<0){")
            .ok_or("incomplete legacy response block")? + start;
        trace.replace_range(start..end, "");
        replace_once(&mut trace, "if(!responseUsed&&finished&&current.medium<0){",
                     "if(finished&&current.medium<0){")?;
    }
    Ok(trace)
}

fn component_entry(original: &str) -> Result<String, String> {
    // Reuse the exact authored material-only BVH traversal, changing its side
    // test for entry. Unlike the scene query this cannot accidentally bake an
    // opaque knob, another panel, or a neighboring component into the response.
    let (start, end) = function(original, "gemControlExit")?;
    let mut entry = original[start..end].to_owned();
    replace_once(&mut entry, "fn gemControlExit(", "fn rubyControlEntry(")?;
    let marker = "    if(interior.z>0.0){";
    let start = entry.find(marker).ok_or("missing control convex intersection")?;
    let opening = start + entry[start..].find('{').ok_or("missing convex body")?;
    let end = block_end(&entry, opening)?;
    entry.replace_range(start..end, CONTROL_PLANES);
    replace_once(&mut entry, "if(dot(ray,normal)>0.0){hit=GemHit(t,normal,medium);}",
                 "if(dot(ray,normal)<0.0){hit=GemHit(t,normal,medium);}")?;
    Ok(entry)
}

fn bake_trace(canonical: &str) -> Result<String, String> {
    let mut trace = canonical.to_owned();
    replace_once(&mut trace, "fn gemTrace(", "fn rubyBakeTrace(")?;
    // Neither coarse text radiance nor coarse text opacity belongs in intrinsic
    // transport. Playback evaluates the first refracted text segment at the
    // drawable sample; the canonical fallback keeps all engraving branches.
    for block in ENGRAVING_BLOCKS {
        let expected = engraving_block(&trace, block);
        replace_once(&mut trace, &expected, "")?;
    }
    if canonical.contains("gemMixerScale(") {
        replace_once(&mut trace, MIXER_ENGRAVING, "")?;
    }
    replace_once(&mut trace,
        "if(current.medium<0){environment=gemEnvironment(current.origin,current.direction);}",
        "// Intrinsic transport deliberately has no external floor or emitter limit.")?;
    replace_once(&mut trace,
        "var hit:GemHit=gemNearest(current.origin,current.direction,current.medium,environment.w);",
        "var hit:GemHit=rubyComponentHit(current.origin,current.direction,current.medium,rubyBakeTarget,1e20);\n        if(event==0&&hit.panel>=0&&hit.distance>0.0){rubyBakeHead=vec4<f32>((current.origin+current.direction*hit.distance-rubyBakePose.xyz)/rubyBakePose.w,1.0);}")?;
    replace_once(&mut trace,
        "var environmentRadiance:vec3<f32>=environment.xyz;\n            if(environmentRadiance.x<0.0){environmentRadiance=gemFloor(current.origin+current.direction*environment.w,-current.direction);}\n            result+=current.weight*environmentRadiance;",
        "rubyRecord(current.origin,current.direction,current.weight,current.depth);")?;
    // Each of these calls appears in both the bubble and dielectric blocks.
    // Only the dielectric block is reachable for a planned ruby componentId.
    let reflected = "gemTerminal(point+reflected*epsilon,reflected)";
    let transmitted = "gemTerminal(point+transmitted*epsilon,transmitted)";
    if trace.matches(reflected).count() != 2 || trace.matches(transmitted).count() != 2 {
        return Err("ruby response terminal markers changed".into());
    }
    trace = trace.replace(reflected, "rubyBakeTerminal(point+reflected*epsilon,reflected,reflectWeight,depth)")
        .replace(transmitted, "rubyBakeTerminal(point+transmitted*epsilon,transmitted,transmitWeight,depth)");
    Ok(trace)
}

fn fast_primary(output: &mut String) -> Result<(), String> {
    let (start, end) = function(output, "gemTrace")?;
    let mut fallback = output[start..end].to_owned();
    // Reuse the authored primary opaque branch, including Edison hover/bypass
    // flags and the monitor/material-specific coat behavior. Keep one source
    // for those equations rather than maintain a second handwritten material.
    let marker = "            if(material.w<0.0001){";
    if fallback.matches(marker).count() != 1 {
        return Err("ruby fast primary opaque branch changed".into());
    }
    let opening = fallback.find(marker).unwrap() + marker.len() - 1;
    let closing = block_end(&fallback, opening)? - 1;
    let mut opaque = fallback[opening+1..closing].to_owned();
    if opaque.contains("result+=current.weight*(opaque+emission);") {
        replace_once(&mut opaque,"result+=current.weight*(opaque+emission);","return vec4<f32>(opaque+emission,0.0);")?;
    }else{replace_once(&mut opaque,
        "result+=current.weight*(opaque+gemRow(base+5).xyz);",
        "return vec4<f32>(opaque+gemRow(base+5).xyz,0.0);")?;}
    replace_once(&mut opaque, "finished=true;", "")?;
    opaque = opaque.replace("-current.direction", "-direction");
    if opaque.contains("current.") || opaque.contains("result+=") {
        return Err("ruby fast primary opaque dependencies changed".into());
    }
    replace_once(&mut fallback, "fn gemTrace(", "fn rubyTraceFallback(")?;
    output.replace_range(start..end, &fallback);
    output.push_str(FAST_PRIMARY_HEAD);
    output.push_str(&opaque);
    output.push_str(FAST_PRIMARY_TAIL);
    output.push_str(PRIMARY_WRAPPER);
    Ok(())
}

fn validate(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| format!("ruby response: {}", error.emit_to_string(source)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module).map_err(|error| format!("ruby response: {error:?}"))?;
    Ok(())
}

pub(crate) fn source(original: &str) -> Result<String, String> {
    source_with_opaque_controls(original,
        std::env::var("WORMWOOD_WORKSTATION_EXTENDED_OPAQUE_CONTROLS").as_deref()==Ok("1"))
}

fn source_with_opaque_controls(original:&str,extended:bool)->Result<String,String> {
    if original.len() > 2 * 1024 * 1024 { return Err("workstation shader exceeds response bound".into()); }
    let canonical = canonical_trace(original)?;
    let entry = component_entry(original)?;
    let bake = bake_trace(&canonical)?;
    let mut playback = canonical;
    replace_once(&mut playback, "        if(finished&&current.medium<0){", PLAYBACK)?;
    let (start, end) = function(original, "gemTrace")?;
    let mut output = original.to_owned();
    output.replace_range(start..end, &playback);
    let (start, end) = function(&output, "gemTerminal")?;
    output.replace_range(start..end, STUDIO_TERMINAL);
    output.push_str(SUPPORT);
    output.push_str(&entry);
    output.push_str(&bake);
    output.push_str(COMPUTE);
    output.push_str(RESOLVE);
    output.push_str(LIVE_ENGRAVING);
    fast_primary(&mut output)?;
    if extended {
        // Current control IDs are 4 + instance*256 + material, instances0..128.
        // Extend only the already canonical opaque primary shortcut. Cache map
        // index16644 belongs to timeline entries and must remain unchanged.
        replace_once(&mut output,
            "if((hit.panel>=0&&hit.panel<16644)||(hit.panel>=700000&&hit.panel<704096)){",
            "if((hit.panel>=0&&hit.panel<32772)||(hit.panel>=700000&&hit.panel<704096)){")?;
    }
    output = convex::transform(&output)?;
    if std::env::var("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT").as_deref()==Ok("1") {
        output = pixel::source(&output)?;
    }
    if std::env::var("WORMWOOD_WORKSTATION_PRIMARY_REUSE").as_deref()==Ok("1") {
        output = primary_reuse::source(&output)?;
    }
    validate(&output)?;
    Ok(output)
}

// Preserve the user-approved exterior policy for cached and uncached terminal
// rays. Live engraving changes only the treatment of embedded text separately.
const STUDIO_TERMINAL: &str = r#"fn gemTerminal(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32> {
    // Exterior policy: resolve the studio directly without scene occlusion.
    let environment=gemEnvironment(origin,ray);
    var radiance=environment.xyz;
    if(radiance.x<0.0){radiance=gemFloor(origin+ray*environment.w,-ray);}
    return vec4<f32>(radiance,1.0);
}"#;

const CONTROL_PLANES: &str = r#"    if(interior.z>0.0){
        var near:f32=-1e20;var far:f32=1e20;var normal:vec3<f32>=vec3<f32>(0.0);
        for(var planeIndex:i32=0;planeIndex<26;planeIndex++){
            if(f32(planeIndex)>=interior.z){break;}
            let plane=gemRow(base+6+planeIndex);let denom=dot(plane.xyz,ray);
            let distance=plane.w-dot(plane.xyz,local);
            if(abs(denom)<0.0000001){
                if(distance< -0.0000001){return hit;}
            }else{
                let t=distance/denom;
                if(denom<0.0&&t>near){near=t;normal=plane.xyz;}
                if(denom>0.0&&t<far){far=t;}
                if(far<near){return hit;}
            }
        }
        if(near*pose.w>0.0000001&&near*pose.w<farLimit&&near<=far){return GemHit(near*pose.w,normal,medium);}
        return hit;
    }"#;

const SUPPORT: &str = r#"
// One shared layout for compute refresh and fragment playback. The render pass
// only reads binding2. Bake writes intrinsic rows0..26 and invalidates27/28;
// the following resolve pass writes lit radiance27 and direct reflection28.
struct RubyResponseData { rows:array<vec4<f32>> };
@group(0) @binding(2) var<storage,read_write> rubyResponse:RubyResponseData;
@group(0) @binding(3) var<storage,read> rubyPlan:RubyResponseData;
var<private> rubyBakeTarget:i32;
var<private> rubyBakePose:vec4<f32>;
var<private> rubyBakeHead:vec4<f32>;
var<private> rubyBakeDropped:vec3<f32>;
var<private> rubyBakeLeaves:i32;
var<private> rubyBakePositions:array<vec4<f32>,8>;
var<private> rubyBakeDirections:array<vec3<f32>,8>;
var<private> rubyBakeWeights:array<vec3<f32>,8>;

fn rubyComponentHit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,componentId:i32,farLimit:f32)->GemHit {
    if(componentId>=0&&componentId<4){
        if(medium>=0){return gemExitPanel(origin,ray,componentId);}
        return gemEnterPanel(origin,ray,gemInverseRay(ray),componentId,farLimit);
    }
    if(componentId>=4&&componentId<16644){
        if(medium>=0){return gemControlExit(origin,ray,componentId,farLimit);}
        return rubyControlEntry(origin,ray,componentId,farLimit);
    }
    return GemHit(farLimit,vec3<f32>(0.0),-1);
}

fn rubyRecord(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32) {
    var slot:i32=0;
    for(var i:i32=1;i<8;i++){
        if(gemMax(rubyBakeWeights[i])<gemMax(rubyBakeWeights[slot])){slot=i;}
    }
    if(gemMax(weight)>gemMax(rubyBakeWeights[slot])){
        rubyBakeDropped+=rubyBakeWeights[slot];
        rubyBakePositions[slot]=vec4<f32>((origin-rubyBakePose.xyz)/rubyBakePose.w,f32(depth));
        rubyBakeDirections[slot]=direction;
        rubyBakeWeights[slot]=weight;
    }else{rubyBakeDropped+=weight;}
    rubyBakeLeaves+=1;
}

fn rubyBakeTerminal(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32)->vec4<f32> {
    // Nonconvex components may reenter themselves. Preserve those branches;
    // escaped lobes resolve the studio directly when played back.
    let hit=rubyComponentHit(origin,direction,-1,rubyBakeTarget,1e20);
    if(hit.panel>=0&&hit.distance>0.0){return vec4<f32>(0.0);}
    rubyRecord(origin,direction,weight,depth);
    return vec4<f32>(0.0,0.0,0.0,1.0);
}

fn rubyResponseSlot(componentId:i32)->i32 {
    // Timeline IDs have a reserved map range but currently keep canonical
    // tracing: their component-only entry traversal has not been implemented.
    if(componentId<0||componentId>=16644||arrayLength(&rubyPlan.rows)<4322u){return -1;}
    if(rubyPlan.rows[0].y<0.5){return -1;}
    let slot=i32(rubyPlan.rows[161u+u32(componentId)/4u][u32(componentId)%4u]);
    if(slot<0||slot>=20||f32(slot)>=rubyPlan.rows[0].x){return -1;}
    let descriptor=rubyPlan.rows[1u+u32(slot)*8u];
    if(i32(descriptor.x)!=componentId||descriptor.w<0.5){return -1;}
    return slot;
}

fn rubyResponseAddress(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,random:vec2<f32>)->i32 {
    if(gemRow(1).w!=0.0||gemRow(0).z!=2.0||direction.x!=0.0||direction.y!=0.0||direction.z!= -1.0){return -1;}
    let slot=rubyResponseSlot(componentId);if(slot<0){return -1;}
    let at=1u+u32(slot)*8u;let descriptor=rubyPlan.rows[at];
    let pose=rubyPlan.rows[at+1u];let bounds=rubyPlan.rows[at+2u];
    if(pose.w<=0.0||bounds.z<=bounds.x||bounds.w<=bounds.y){return -1;}
    let local=(origin.xy-pose.xy)/pose.w;
    let uv=vec2<f32>((local.x-bounds.x)/(bounds.z-bounds.x),(bounds.w-local.y)/(bounds.w-bounds.y));
    if(any(uv<vec2<f32>(0.0))||any(uv>=vec2<f32>(1.0))){return -1;}
    let edge=i32(descriptor.y);if(edge!=64&&edge!=128){return -1;}
    let texel=clamp(vec2<i32>(uv*f32(edge)),vec2<i32>(0),vec2<i32>(edge-1));
    var sample:i32=0;if(random.x>0.5){sample=1;}
    let address=i32(descriptor.z)+(texel.y*edge+texel.x)*60+sample*30;
    if(address<0||u32(address)+29u>=arrayLength(&rubyResponse.rows)){return -1;}
    if(rubyResponse.rows[u32(address)].w<0.5){return -1;}
    if(rubyResponse.rows[u32(address)+27u].w<0.5||rubyResponse.rows[u32(address)+28u].w<0.5){return -1;}
    return address;
}
"#;

const PLAYBACK: &str = r#"        var rubyUsed:bool=false;
        if(event==0&&!finished){
            let responseAddress=rubyResponseAddress(current.origin,current.direction,hit.panel,random);
            if(responseAddress>=0){
                let address=u32(responseAddress);
                result+=current.weight*rubyLiveEngraving(current.origin,current.direction,hit,random,rubyResponse.rows[address+27u].xyz);
                rubyUsed=true;finished=true;
            }
        }
        if(!rubyUsed&&finished&&current.medium<0){"#;

// Both reviewed canonical layouts are accepted; any other engraving flow
// remains an error so intrinsic transport cannot silently cache live text.
fn engraving_block(source: &str, block: &str) -> String {
    if source.contains(block) { block.to_owned() } else {
        block.replace("if(inside&&current.medium>=4){", "if(inside&&current.medium>=4&&current.medium<800000){")
    }
}

const MIXER_ENGRAVING: &str = r#"            if(inside&&current.medium>=800000&&current.medium<853760){
                var engraving:vec4<f32>=gemMixerEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
"#;

const ENGRAVING_BLOCKS: [&str; 3] = [
    r#"            if(inside&&current.medium<4){
                var engraving:vec4<f32>=gemEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
"#,
    r#"            if(inside&&current.medium>=4){
                var engraving:vec4<f32>=gemControlEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
                var navigation:vec4<f32>=gemPluginNavigationEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*navigation.xyz;current.weight*=navigation.w;
            }
"#,
    r#"            if(inside&&current.medium>=700000&&current.medium<704096){
                var engraving:vec4<f32>=gemTimelineEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
"#,
];

const COMPUTE: &str = r#"
@compute @workgroup_size(8,8,1)
fn cs_ruby_response(@builtin(global_invocation_id) id:vec3<u32>) {
    if(arrayLength(&rubyPlan.rows)<161u||id.z>=20u||f32(id.z)>=rubyPlan.rows[0].x||rubyPlan.rows[0].y<0.5){return;}
    let at=1u+id.z*8u;let descriptor=rubyPlan.rows[at];
    let edge=u32(descriptor.y);
    if(descriptor.w!=2.0||(edge!=64u&&edge!=128u)||id.x>=edge||id.y>=edge){return;}
    rubyBakeTarget=i32(descriptor.x);
    if(rubyBakeTarget<0||rubyBakeTarget>=16644){return;}
    rubyBakePose=rubyPlan.rows[at+1u];
    if(rubyBakePose.w<=0.0){return;}
    let bounds=rubyPlan.rows[at+2u];
    let uv=(vec2<f32>(id.xy)+vec2<f32>(0.5))/f32(edge);
    let xy=vec2<f32>(mix(bounds.x,bounds.z,uv.x),mix(bounds.w,bounds.y,uv.y));
    let origin=vec3<f32>(rubyBakePose.xy+xy*rubyBakePose.w,rubyPlan.rows[at+3u].x);
    let first=u32(descriptor.z)+(id.y*edge+id.x)*60u;
    if(first+59u>=arrayLength(&rubyResponse.rows)){return;}
    for(var sample:u32=0u;sample<2u;sample++){
        rubyBakeHead=vec4<f32>(0.0);rubyBakeDropped=vec3<f32>(0.0);rubyBakeLeaves=0;
        for(var lobe:i32=0;lobe<8;lobe++){
            rubyBakePositions[lobe]=vec4<f32>(0.0);
            rubyBakeDirections[lobe]=vec3<f32>(0.0);rubyBakeWeights[lobe]=vec3<f32>(0.0);
        }
        let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sample));
        let radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        let address=first+sample*30u;
        rubyResponse.rows[address]=rubyBakeHead;
        rubyResponse.rows[address+1u]=vec4<f32>(rubyBakeDropped,f32(rubyBakeLeaves));
        for(var lobe:u32=0u;lobe<8u;lobe++){
            let leaf=address+2u+3u*lobe;
            rubyResponse.rows[leaf]=rubyBakePositions[lobe];
            rubyResponse.rows[leaf+1u]=vec4<f32>(rubyBakeDirections[lobe],0.0);
            rubyResponse.rows[leaf+2u]=vec4<f32>(rubyBakeWeights[lobe],0.0);
        }
        rubyResponse.rows[address+26u]=vec4<f32>(radiance,0.0);
        rubyResponse.rows[address+27u]=vec4<f32>(0.0);
        rubyResponse.rows[address+28u]=vec4<f32>(0.0);
        rubyResponse.rows[address+29u]=vec4<f32>(0.0);
    }
}
"#;

const RESOLVE: &str = r#"
// Run after any dirty intrinsic bake, before every draw with response playback.
// Each invocation owns one texel. A separate pass boundary makes its writes
// visible to the renderer; lighting never changes intrinsic transport rows.
@compute @workgroup_size(8,8,1)
fn cs_ruby_resolve(@builtin(global_invocation_id) id:vec3<u32>) {
    if(arrayLength(&rubyPlan.rows)<161u||id.z>=20u||f32(id.z)>=rubyPlan.rows[0].x||rubyPlan.rows[0].y<0.5){return;}
    let at=1u+id.z*8u;let descriptor=rubyPlan.rows[at];
    let edge=u32(descriptor.y);
    if(descriptor.w<0.5||(edge!=64u&&edge!=128u)||id.x>=edge||id.y>=edge){return;}
    let componentId=i32(descriptor.x);
    if(componentId<0||componentId>=16644){return;}
    let pose=rubyPlan.rows[at+1u];
    if(pose.w<=0.0){return;}
    let first=u32(descriptor.z)+(id.y*edge+id.x)*60u;
    if(first+59u>=arrayLength(&rubyResponse.rows)){return;}
    for(var sample:u32=0u;sample<2u;sample++){
        let address=first+sample*30u;
        rubyResponse.rows[address+27u]=vec4<f32>(0.0);
        rubyResponse.rows[address+28u]=vec4<f32>(0.0);
        if(rubyResponse.rows[address].w<0.5){continue;}
        var result=vec3<f32>(0.0);
        var front=vec3<f32>(0.0);
        result+=rubyResponse.rows[address+26u].xyz;
        for(var lobe:u32=0u;lobe<8u;lobe++){
            let leaf=address+2u+3u*lobe;
            let weight=rubyResponse.rows[leaf+2u].xyz;
            let priority=gemMax(weight);if(priority<=0.0){continue;}
            let position=rubyResponse.rows[leaf];
            let exitOrigin=pose.xyz+position.xyz*pose.w;
            let exitDirection=gemSafeUnit(rubyResponse.rows[leaf+1u].xyz);
            let terminal=gemTerminal(exitOrigin,exitDirection);
            result+=weight*terminal.xyz;
            if(i32(position.w)==1){front+=weight*terminal.xyz;}
        }
        rubyResponse.rows[address+27u]=vec4<f32>(result,1.0);
        rubyResponse.rows[address+28u]=vec4<f32>(front,1.0);
    }
}
"#;

const LIVE_ENGRAVING: &str = r#"
// Only owners of the actual authored engraving can enter this path. Extended
// palette material+2.w values have unrelated meanings and must not be addresses.
fn rubyLiveTextOwner(componentId:i32)->bool {
    if(componentId==2||componentId==3){return gemRow(212).w>=0.5;}
    if(componentId<4||componentId>=16644){return false;}
    let instance=(componentId-4)/256;
    if(instance>=61&&instance<65){return true;}
    if(instance>=10||gemRow(244).y<0.5){return false;}
    let base=gemPanelRow(componentId);
    return gemRow(base+2).w>0.0&&gemRow(base).w>=0.0001;
}

// Use the canonical text boxes before paying for an exact component exit. An
// infinite limit only retains extra candidates; the exit still clips lettering.
fn rubyLiveTextSegmentMayHit(origin:vec3<f32>,ray:vec3<f32>,componentId:i32)->bool {
    if(componentId==2||componentId==3){
        let pixel=2.0*gemRow(3).w/gemRow(8).y;
        let group=218+2*(componentId-2);
        return gemBox(origin,gemInverseRay(ray),gemRow(group).xyz,
            gemRow(group+1).xyz+vec3<f32>(pixel,pixel,0.0000001),1e20);
    }
    let instance=(componentId-4)/256;
    if(instance>=61&&instance<65){return true;}
    let descriptor=i32(gemRow(gemPanelRow(componentId)+2).w);
    let pose=gemRow(gemControlPoseRow(instance));
    let region=gemRow(descriptor);let depth=gemRow(descriptor+1);
    let pixel=2.0*gemRow(3).w/(gemRow(8).y*pose.w);
    let center=vec3<f32>(region.xy+region.zw*0.5,(depth.x+depth.z)*0.5);
    let extent=vec3<f32>(region.zw*0.5+vec2<f32>(pixel),(depth.x-depth.z)*0.5+0.0000001);
    return gemBox((origin-pose.xyz)/pose.w,gemInverseRay(ray),center,extent,1e20);
}

fn rubyLiveFrontReflection(origin:vec3<f32>,direction:vec3<f32>,componentId:i32,
                           random:vec2<f32>)->vec3<f32> {
    let responseAddress=rubyResponseAddress(origin,direction,componentId,random);
    if(responseAddress<0){return vec3<f32>(0.0);}
    return rubyResponse.rows[u32(responseAddress)+28u].xyz;
}

// Explicit approximation: only the first refracted segment acquires engraving.
// The cache contains no coarse engraved radiance or opacity. All later glass
// branches remain cached, including their original absorption and self-hits.
fn rubyLiveEngraving(origin:vec3<f32>,direction:vec3<f32>,hit:GemHit,
                     random:vec2<f32>,cachedRGB:vec3<f32>)->vec3<f32> {
    if(!rubyLiveTextOwner(hit.panel)||hit.distance<=0.0){return cachedRGB;}
    let base=gemPanelRow(hit.panel);
    let material=gemRow(base);
    if(material.w<0.0001||material.y<=0.0||gemRow(8).z<=1.0){return cachedRGB;}
    let n=hit.normal;
    // At the primary event, depth and event are zero, so canonical xi=random.
    let micro=gemMicroNormal(n,-direction,material.z,random);
    let ray=refract(direction,micro,1.0/material.y);
    if(dot(ray,n)>=0.0||dot(ray,ray)<0.5){return cachedRGB;}
    let f=gemFresnel(dot(-direction,micro),1.0,material.y);
    let alpha=max(material.z*material.z,0.00001);
    let weight=((1.0-f)*material.w/(material.y*material.y))
        *gemRow(base+4).xyz
        *gemSampleMask(dot(-direction,n),dot(ray,n),alpha)
        /gemEnergy(hit.panel,false,dot(-direction,n));
    if(gemMax(weight)<=0.00005){return cachedRGB;}
    let point=origin+direction*hit.distance;
    let dimensions=gemRow(base+3).xyz;
    let epsilon=max(0.0000002,min(min(dimensions.x,dimensions.y),dimensions.z)*0.00001);
    let inside=point+ray*epsilon;
    if(!rubyLiveTextSegmentMayHit(inside,ray,hit.panel)){return cachedRGB;}
    let exitHit=rubyComponentHit(inside,ray,hit.panel,hit.panel,1e20);
    if(exitHit.panel!=hit.panel||exitHit.distance<=0.0){return cachedRGB;}
    var engraving=vec4<f32>(0.0,0.0,0.0,1.0);
    if(hit.panel<4){
        engraving=gemEngraving(inside,ray,hit.panel,exitHit.distance);
    }else{
        engraving=gemControlEngraving(inside,ray,hit.panel,exitHit.distance);
        let navigation=gemPluginNavigationEngraving(inside,ray,hit.panel,exitHit.distance);
        engraving=vec4<f32>(engraving.xyz+engraving.w*navigation.xyz,
                            engraving.w*navigation.w);
    }
    if(engraving.w==1.0&&all(engraving.xyz==vec3<f32>(0.0))){return cachedRGB;}
    let front=rubyLiveFrontReflection(origin,direction,hit.panel,random);
    // Direct depth1 reflected leaves never entered the glass. Protect those
    // from text opacity. The remainder includes concave reentry reflections;
    // treating those as behind this first text segment is an approximation.
    let behind=max(cachedRGB-front,vec3<f32>(0.0));
    return front+engraving.w*behind+weight*engraving.xyz;
}
"#;

const FAST_PRIMARY_HEAD: &str = r#"
// Resolved ruby, studio misses and primary opaque hits need no ray work queue.
// Negative alpha is an internal fallback sentinel; drawable alpha stays one.
fn rubyFastTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec4<f32> {
    if(gemRow(8).w<1.0){return vec4<f32>(0.0,0.0,0.0,-1.0);}
    let environment=gemEnvironment(origin,direction);
    let hit=gemNearest(origin,direction,-1,environment.w);
    if(hit.panel<0||hit.distance<=0.0){
        var radiance=environment.xyz;
        if(radiance.x<0.0){radiance=gemFloor(origin+direction*environment.w,-direction);}
        return vec4<f32>(radiance,0.0);
    }
    let responseAddress=rubyResponseAddress(origin,direction,hit.panel,random);
    if(responseAddress>=0){
        let cached=rubyResponse.rows[u32(responseAddress)+27u].xyz;
        return vec4<f32>(rubyLiveEngraving(origin,direction,hit,random,cached),1.0);
    }
    if((hit.panel>=0&&hit.panel<16644)||(hit.panel>=700000&&hit.panel<704096)){
        let base=gemPanelRow(hit.panel);let material=gemRow(base);
        if(material.w<0.0001){
            let point=origin+direction*hit.distance;let n=hit.normal;
"#;

const FAST_PRIMARY_TAIL: &str = r#"
        }
    }
    if(hit.panel>=90000&&hit.panel<90064){
        return vec4<f32>(gemAudioSphereShade(origin+direction*hit.distance,-direction,hit.normal,hit.panel),0.0);
    }
    // Bubbles, imported materials, unsupported glass and uncached ruby retain
    // the complete integrator, including the established studio terminal policy.
    return vec4<f32>(0.0,0.0,0.0,-1.0);
}
"#;

const PRIMARY_WRAPPER: &str = r#"
fn gemTrace(origin:vec3<f32>,direction:vec3<f32>,random:vec2<f32>)->vec3<f32> {
    let fastSample=rubyFastTrace(origin,direction,random);
    if(fastSample.w>=0.0){
        if(fastSample.w>0.5){
            // RUBY_PRIMARY_CACHED
        }
        return fastSample.xyz;
    }
    return rubyTraceFallback(origin,direction,random);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstation_ruby_response_generates_valid_compute_and_fragment_entries() {
        let result = source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
        // source() validates the complete shader, including resource usage for
        // both compute passes and the fragment path that reads their results.
        let module = naga::front::wgsl::parse_str(&result).unwrap();
        for (name, stage) in [
            ("cs_ruby_response", naga::ShaderStage::Compute),
            ("cs_ruby_response_general", naga::ShaderStage::Compute),
            ("cs_ruby_resolve", naga::ShaderStage::Compute),
            ("fs_main", naga::ShaderStage::Fragment),
        ] {
            assert!(module.entry_points.iter().any(|entry| entry.name == name && entry.stage == stage));
        }
        assert!(result.contains("fn rubyControlEntry("));
        assert!(result.contains("if(componentId>=0&&componentId<4)"));
        let (start, end) = function(&result, "rubyTraceFallback").unwrap();
        assert!(!result[start..end].contains("gemResponseCompatible"));
        assert!(!result[start..end].contains("rubyBakeTrace"));
        assert!(result[start..end].contains("gemNearest(current.origin,current.direction,current.medium,environment.w)"));
        let (start, end) = function(&result, "gemTerminal").unwrap();
        let exterior = &result[start..end];
        assert!(exterior.contains("gemEnvironment(origin,ray)"));
        assert!(exterior.contains("gemFloor(origin+ray*environment.w,-ray)"));
        assert!(exterior.contains("return vec4<f32>(radiance,1.0)"));
        assert!(!exterior.contains("gemAny("));
        assert!(!exterior.contains("gemNearest("));
        assert!(!exterior.contains("Hit("));
        let (start, end) = function(&result, "rubyBakeTrace").unwrap();
        assert!(!result[start..end].contains("gemNearest("));
        assert!(!result[start..end].contains("gemTerminal("));
        assert!(!result[start..end].contains("rubyResponseAddress("));
    }

    #[test]
    fn workstation_ruby_response_separates_intrinsic_transport_from_live_and_fallback_text() {
        let result = source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
        let (start, end) = function(&result, "rubyBakeTrace").unwrap();
        let bake = &result[start..end];
        for name in ["gemEngraving(", "gemControlEngraving(", "gemPluginNavigationEngraving(", "gemTimelineEngraving(", "gemMixerEngraving("] {
            assert!(!bake.contains(name), "intrinsic transport must not cache text: {name}");
        }
        let (start, end) = function(&result, "rubyTraceFallback").unwrap();
        let fallback = &result[start..end];
        // A cache miss still uses all original engraving segments, including
        // their absorption of subsequent radiance; text is not silently lost.
        for block in ENGRAVING_BLOCKS {
            assert!(fallback.contains(&engraving_block(fallback, block)));
        }
        if fallback.contains("gemMixerScale(") {
            assert!(fallback.contains(MIXER_ENGRAVING));
        }
        let (start, end) = function(&result, "rubyLiveEngraving").unwrap();
        let live = &result[start..end];
        assert!(live.contains("gemEngraving(inside,ray,hit.panel,exitHit.distance)"));
        assert!(live.contains("gemControlEngraving(inside,ray,hit.panel,exitHit.distance)"));
        assert!(live.contains("gemPluginNavigationEngraving(inside,ray,hit.panel,exitHit.distance)"));
    }

    #[test]
    fn workstation_ruby_response_primary_path_keeps_complete_fallback_and_opaque_flags() {
        let result = source(include_str!("../Procedures/Shaders/workstation.wgsl")).unwrap();
        let (start, end) = function(&result, "rubyFastTrace").unwrap();
        let fast = &result[start..end];
        assert!(!fast.contains("pending["));
        assert!(fast.contains("gemNearest(origin,direction,-1,environment.w)"));
        assert!(fast.contains("if(instance>=13&&instance<29)"));
        assert!(fast.contains("flags>=4.0"));
        assert!(fast.contains("gemOpaqueSurface(point,-direction,n,surfaceColor"));
        let (start, end) = function(&result, "gemTrace").unwrap();
        let wrapper = &result[start..end];
        assert!(wrapper.contains("return rubyTraceFallback(origin,direction,random);"));
        assert_eq!(result.matches("// RUBY_PRIMARY_CACHED").count(), 1);
        assert!(wrapper.contains("if(fastSample.w>0.5){\n            // RUBY_PRIMARY_CACHED"));
        let (start, end) = function(&result, "rubyTraceFallback").unwrap();
        let fallback = &result[start..end];
        for call in ["userSurfaceLabel(", "gemFilmReflectance(", "gemTimelineEngraving("] {
            assert!(fallback.contains(call), "unsupported primary material lost its full fallback: {call}");
        }
    }

    #[test]
    fn workstation_ruby_response_rejects_changed_integrator_markers() {
        let original = include_str!("../Procedures/Shaders/workstation.wgsl");
        assert!(source(&original.replace("fn gemTrace(", "fn changedTrace(")).is_err());
        assert!(source(&original.replace("fn gemTerminal(", "fn changedTerminal(")).is_err());
        let canonical = canonical_trace(original).unwrap();
        for block in ENGRAVING_BLOCKS {
            let expected = engraving_block(&canonical, block);
            assert!(canonical.contains(&expected));
            assert!(bake_trace(&canonical.replace(&expected, "")).is_err(),
                "changed engraving flow must be reviewed before it can be removed from the bake");
        }
        if canonical.contains("gemMixerScale(") {
            assert!(bake_trace(&canonical.replace(MIXER_ENGRAVING, "")).is_err());
        }
        assert!(source("invalid shader").is_err());
    }
}

#[cfg(test)]mod extended_opaque_control_tests {
    use super::*;
    #[test]fn extended_control_shortcut_changes_only_opaque_gate_and_validates(){
        let original=include_str!("../Procedures/Shaders/workstation.wgsl");
        let baseline=source_with_opaque_controls(original,false).unwrap();
        let candidate=source_with_opaque_controls(original,true).unwrap();
        // Both complete generated programs were parsed and Naga-validated by
        // source_with_opaque_controls, including opt-in primary reuse if active.
        let old="if((hit.panel>=0&&hit.panel<16644)||(hit.panel>=700000&&hit.panel<704096)){";
        let new="if((hit.panel>=0&&hit.panel<32772)||(hit.panel>=700000&&hit.panel<704096)){";
        assert_eq!(baseline.matches(old).count(),1);
        assert_eq!(candidate,baseline.replacen(old,new,1));
        assert!(candidate.contains("if(componentId<0||componentId>=16644||arrayLength(&rubyPlan.rows)<4322u)"));
        assert!(candidate.contains("if(componentId>=4&&componentId<16644)"));
    }
    #[test]fn extended_opaque_body_is_exact_original_primary_body(){
        let original=include_str!("../Procedures/Shaders/workstation.wgsl");
        let canonical=canonical_trace(original).unwrap();
        let marker="            if(material.w<0.0001){";
        let open=canonical.find(marker).unwrap()+marker.len()-1;
        let end=block_end(&canonical,open).unwrap()-1;
        let mut expected=canonical[open+1..end].to_owned();
        // At primary event0: medium=-1, weight=(1,1,1), result=(0,0,0),
        // pending count0. The opaque branch ends that path without spawning rays.
        replace_once(&mut expected,"result+=current.weight*(opaque+emission);",
            "return vec4<f32>(opaque+emission,0.0);").unwrap();
        replace_once(&mut expected,"finished=true;","").unwrap();
        expected=expected.replace("-current.direction","-direction");
        let candidate=source_with_opaque_controls(original,true).unwrap();
        let (start,end)=function(&candidate,"rubyFastTrace").unwrap();
        assert!(candidate[start..end].contains(&expected));
        assert!(!expected.contains("current."));assert!(!expected.contains("pending"));
        for call in ["gemMixerMeterColor(point,hit.panel)","gemOpaqueSurface(","gemOpaque("] {
            assert!(expected.contains(call));
        }
    }
    #[test]fn extended_gate_is_exact_current_control_family_without_other_ids(){
        let original=include_str!("../Procedures/Shaders/workstation.wgsl");
        assert!(original.contains("else if(instance>=128){break;}"));
        assert!(original.contains("var material:i32=4+instance*256+i32(a.w);"));
        assert_eq!(4+65*256,16644);assert_eq!(4+128*256,32772);
        for instance in 0..128 {for material in 0..256 {
            let id=4+instance*256+material;assert!((4..32772).contains(&id));
            assert_eq!(id<16644,instance<65);
        }}
        for id in [32772,90000,100000,200000,719997,720000,800000] {
            assert!(!(0..32772).contains(&id));
        }
    }
}
