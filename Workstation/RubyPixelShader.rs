//! Opt-in exact screen-pixel transport for original glass controls and timeline handles.
//!
//! This module is the Rust port of ruby_pixel_cache.py with engraved=True.
//! It only assembles source; the caller chooses whether to enable this mode
//! and validates the complete WGSL before replacing any renderer pipeline.
//! The matching CPU plan must invalidate on pose, camera, resolution, geometry,
//! material, engraving descriptor/density, text parameters and engraving light
//! changes. Legacy ruby transport and its live text remain unchanged.
//!
//! Each pixel/sample stores 16 chronological event records. Original world
//! terminal origins and directions occupy xyz; the three spare w lanes hold
//! that event's engraved RGB. Resolve adds engraving before the exterior lobe,
//! preserving the canonical sum order even for TIR events without a lobe.
//! Descriptor mode1 uses 108 rows/pixel in an allocated span of the existing
//! control region. Row6.x records the span length; larger spans do not grow the
//! shared response buffer. The CPU plan proves single-material closed handles.

fn once(source: &str, old: &str, new: &str) -> Result<String, String> {
    if old.is_empty() || source.matches(old).count() != 1 {
        return Err(format!("pixel cache marker changed: {}", old.lines().next().unwrap_or(old)));
    }
    Ok(source.replacen(old, new, 1))
}

fn closing(source: &str, opening: usize) -> Result<usize, String> {
    if source.as_bytes().get(opening) != Some(&b'{') {
        return Err("pixel cache block opening changed".into());
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
    if depth != 0 { return Err("pixel cache block is unclosed".into()); }
    Ok(end)
}

fn span(source: &str, marker: &str) -> Result<(usize, usize), String> {
    if source.matches(marker).count() != 1 {
        return Err(format!("pixel cache block changed: {marker}"));
    }
    let start = source.find(marker).ok_or("missing pixel cache block")?;
    let opening = start + source[start..].find('{').ok_or("missing pixel cache opening")?;
    Ok((start, closing(source, opening)?))
}

fn function<'a>(source: &'a str, name: &str) -> Result<&'a str, String> {
    let (start, end) = span(source, &format!("fn {name}("))?;
    Ok(&source[start..end])
}

fn edit_function(source: &str, name: &str, old: &str, new: &str) -> Result<String, String> {
    let original = function(source, name)?;
    once(source, original, &once(original, old, new)?)
}

const BEER: &str = "            if(inside){current.weight*=exp(-gemRow(base+1).xyz*hit.distance/gemObjectScale(hit.panel));}\n";
const MIXER_BEER:&str="            if(inside){var opticalDistance:f32=hit.distance/gemObjectScale(hit.panel);\n                if(current.medium>=800000&&current.medium<853760){opticalDistance=hit.distance*length(current.direction/gemMixerScale((current.medium-800000)/256));}\n                current.weight*=exp(-gemRow(base+1).xyz*opticalDistance);}\n";
const NAVIGATION_GUARD: &str = "if(instance<61||instance>=65||abs(ray.z)<0.00001){return vec4<f32>(0.0,0.0,0.0,1.0);}";
const CONTROL_GUARD: &str = "if(medium<4||instance>=10||gemRow(244).y<0.5||abs(ray.z)<0.00001){return vec4<f32>(0.0,0.0,0.0,1.0);}";
const TIMELINE_GUARD: &str = "if(medium<700000||medium>=704096||abs(ray.z)<0.00001){return vec4<f32>(0.0,0.0,0.0,1.0);}";

const LEGACY_SLOT_GUARD: &str = "    if(componentId<0||componentId>=16644||arrayLength(&rubyPlan.rows)<4322u){return -1;}";
const PIXEL_SLOT_GUARD: &str = r#"    if(componentId<0||arrayLength(&rubyPlan.rows)<4322u){return -1;}
    let timeline=componentId>=700000&&componentId<704096;
    var mapping=componentId;
    if(timeline){mapping=16644+(componentId-700000);}
    else if(componentId>=16644){return -1;}
    let mapRow=161u+u32(mapping)/4u;
    if(mapRow>=arrayLength(&rubyPlan.rows)){return -1;}"#;

fn timeline_slots(source: &str) -> Result<String, String> {
    let slot = function(source, "rubyResponseSlot")?;
    let slot = once(slot,
        "    // Timeline IDs have a reserved map range but currently keep canonical\n    // tracing: their component-only entry traversal has not been implemented.\n",
        "    // Timeline handles occupy the reserved map range in exact pixel mode.\n")?;
    let slot = once(&slot, LEGACY_SLOT_GUARD, PIXEL_SLOT_GUARD)?;
    let slot = once(&slot,
        "    let slot=i32(rubyPlan.rows[161u+u32(componentId)/4u][u32(componentId)%4u]);",
        "    let slot=i32(rubyPlan.rows[mapRow][u32(mapping)%4u]);")?;
    let slot = once(&slot,
        "    return slot;",
        "    if(timeline&&rubyPlan.rows[1u+u32(slot)*8u+4u].w!=1.0){return -1;}\n    return slot;")?;
    once(source, function(source, "rubyResponseSlot")?, &slot)
}

fn full_engraving(source: &str, trace: &str, support: &str) -> Result<(String, String), String> {
    let fallback = function(source, "rubyTraceFallback")?;
    let control=if fallback.contains("            if(inside&&current.medium>=4&&current.medium<800000){"){"            if(inside&&current.medium>=4&&current.medium<800000){"}else{"            if(inside&&current.medium>=4){"};
    let beer=if fallback.contains(MIXER_BEER){MIXER_BEER}else{BEER};
    let (start, end) = span(fallback,control)?;
    let engraving = format!("{}\n", &fallback[start..end]);
    let engraving = once(&engraving,
        "                result+=current.weight*engraving.xyz;current.weight*=engraving.w;",
        "                let engravedContribution=current.weight*engraving.xyz;\n\
                 \x20               rubyPixelEmission[rubyPixelEvent]=engravedContribution;\n\
                 \x20               result+=engravedContribution;current.weight*=engraving.w;")?;
    for (name, guard) in [("gemControlEngraving", CONTROL_GUARD),
                          ("gemPluginNavigationEngraving", NAVIGATION_GUARD),
                          ("gemTimelineEngraving", TIMELINE_GUARD)] {
        if !function(source, name)?.contains(guard) {
            return Err(format!("engraving ownership changed: {name}"));
        }
    }
    let (timeline_start, timeline_end) = span(fallback,
        "            if(inside&&current.medium>=700000&&current.medium<704096){")?;
    let timeline = format!("{}\n", &fallback[timeline_start..timeline_end]);
    let timeline = once(&timeline,
        "                result+=current.weight*engraving.xyz;current.weight*=engraving.w;",
        "                let timelineContribution=current.weight*engraving.xyz;\n\
                 \x20               rubyPixelEmission[rubyPixelEvent]=timelineContribution;\n\
                 \x20               result+=timelineContribution;current.weight*=engraving.w;")?;
    if fallback.matches(beer).count() != 1 || timeline_start <= end
        || fallback.find(beer).is_none_or(|at| at <= timeline_end) {
        return Err("canonical engraving/Beer operation order changed".into());
    }
    // These ID families are disjoint: for a timeline handle the original
    // control and navigation calls return exactly (0,0,0,1). Retain those
    // canonical operations, then record the timeline contribution in this
    // event's RGB lanes. Every text layer keeps its own partial Beer term.
    // Both blocks stay BEFORE the separate full-segment Beer update.
    let trace = once(trace, beer, &format!("{engraving}{timeline}{beer}"))?;
    let event = "        if(f32(event)>=gemRow(8).w){break;}\n";
    let trace = once(&trace, event, &format!("{event}{}",
        "        if(event>=16){rubyPixelHead.w=0.0;rubyPixelDropped=vec3<f32>(1.0);return vec3<f32>(0.0);}\n\
         \x20       rubyPixelEvent=event;rubyPixelLeaves=event+1;\n"))?;
    let support = once(support,
        "var<private> rubyPixelLeaves:i32;",
        "var<private> rubyPixelLeaves:i32;\nvar<private> rubyPixelEvent:i32;\nvar<private> rubyPixelEmission:array<vec3<f32>,16>;")?;
    let support = once(&support,
        "||gemRow(base+2).w!=0.0){return false;}",
        "||gemRow(base+2).w<0.0){return false;}")?;
    let old_record = function(&support, "rubyPixelRecord")?;
    let support = once(&support, old_record, ENGRAVED_RECORD)?;
    let support = once(&support,
        "            rubyPixelPositions[lobe]=vec4<f32>(0.0);",
        "            rubyPixelPositions[lobe]=vec4<f32>(0.0);\n            rubyPixelEmission[lobe]=vec3<f32>(0.0);")?;
    let support = once(&support, LEAF_WRITE, ENGRAVED_LEAF_WRITE)?;
    let support = once(&support,
        "        result+=rubyResponse.rows[address+50u].xyz;\n",
        "        // Intrinsic row50 is diagnostic: event RGB is accumulated below\n\
         \x20       // in canonical order, interleaved with each terminal contribution.\n")?;
    let support = once(&support,
        "            let weight=rubyResponse.rows[leaf+2u].xyz;",
        "            let weight=rubyResponse.rows[leaf+2u].xyz;\n\
         \x20           let emission=vec3<f32>(rubyResponse.rows[leaf].w,rubyResponse.rows[leaf+1u].w,rubyResponse.rows[leaf+2u].w);\n\
         \x20           result+=emission;\n\
         \x20           if(gemMax(weight)<=0.0){continue;}")?;
    let support = once(&support,
        "            if(i32(position.w)==1){front+=weight*terminal.xyz;}",
        "            if(lobe==0u){front+=weight*terminal.xyz;}")?;
    Ok((trace, support))
}

/// Transform a completed guarded-convex ruby shader. Both unengraved and
/// engraved mode1 controls and timeline handles use exact event transport. No partial output escapes
/// on Err; the caller must retain its existing validated pipeline then.
pub(crate) fn source(original: &str) -> Result<String, String> {
    if original.len() > 2 * 1024 * 1024 || original.contains("fn rubyPixel") {
        return Err("unexpected pixel-cache input".into());
    }
    let terminal = function(original, "gemTerminal")?;
    for marker in ["let environment=gemEnvironment(origin,ray);",
                   "gemFloor(origin+ray*environment.w,-ray)",
                   "return vec4<f32>(radiance,1.0);"] {
        if terminal.matches(marker).count() != 1 {
            return Err("pixel cache requires the installed studio policy".into());
        }
    }
    if terminal.contains("gemNearest(") || terminal.contains("gemAny(") {
        return Err("pixel cache requires direct studio termination".into());
    }
    let trace = function(original, "rubyBakeTrace")?;
    for marker in ["gemEngraving(", "gemControlEngraving(", "gemPluginNavigationEngraving(",
                   "gemTimelineEngraving(", "rubyResponseAddress(", "gemTerminal("] {
        if trace.contains(marker) {
            return Err(format!("unexpected intrinsic trace operation: {marker}"));
        }
    }
    if trace.matches("rubyBakeTerminal(").count() != 4 || trace.matches("rubyRecord(").count() != 1 {
        return Err("intrinsic terminal branch structure changed".into());
    }
    let trace = once(trace, "fn rubyBakeTrace(", "fn rubyPixelTrace(")?;
    let mut trace = once(&trace,
        "rubyComponentHit(current.origin,current.direction,current.medium,rubyBakeTarget,1e20)",
        "rubyPixelComponentHit(current.origin,current.direction,current.medium,rubyBakeTarget,1e20)")?;
    for (old, new) in [("rubyBakeTerminal", "rubyPixelTerminal"), ("rubyRecord", "rubyPixelRecord"),
                       ("rubyBakeTarget", "rubyPixelTarget"), ("rubyBakePose", "rubyPixelPose"),
                       ("rubyBakeHead", "rubyPixelHead")] {
        trace = trace.replace(old, new);
    }
    let entry = function(original, "gemControlHit")?;
    let entry = once(entry, "fn gemControlHit(", "fn rubyPixelControlEntry(")?;
    let entry = once(&entry,
        "        var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);",
        "        if(instance!=(rubyPixelTarget-4)/256){continue;}\n        var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);")?;
    let entry = once(&entry,
        "                var material:i32=4+instance*256+i32(a.w);",
        "                var material:i32=4+instance*256+i32(a.w);\n                if(material!=rubyPixelTarget){continue;}")?;
    let (trace, support) = full_engraving(original, &trace, SUPPORT)?;
    let descriptor = "    let at=1u+id.z*8u;let descriptor=rubyPlan.rows[at];\n";
    let output = timeline_slots(original)?;
    let output = edit_function(&output, "cs_ruby_response_general", descriptor,
        &format!("{descriptor}    if(rubyPlan.rows[at+4u].w==1.0){{rubyPixelBake(id,at);return;}}\n"))?;
    let output = edit_function(&output, "cs_ruby_resolve", descriptor,
        &format!("{descriptor}    if(rubyPlan.rows[at+4u].w==1.0){{rubyPixelResolve(id,at);return;}}\n"))?;
    let descriptor = "    let at=1u+u32(slot)*8u;let descriptor=rubyPlan.rows[at];\n";
    let output = edit_function(&output, "rubyResponseAddress", descriptor,
        &format!("{descriptor}    if(rubyPlan.rows[at+4u].w==1.0){{return rubyPixelAddress(origin,componentId,random,at);}}\n"))?;
    let output = edit_function(&output, "rubyTraceFallback",
        "rubyResponse.rows[address+27u].xyz", "rubyResponse.rows[address+rubyResolvedOffset(hit.panel)].xyz")?;
    let output = edit_function(&output, "rubyFastTrace",
        "rubyResponse.rows[u32(responseAddress)+27u].xyz", "rubyResponse.rows[u32(responseAddress)+rubyResolvedOffset(hit.panel)].xyz")?;
    let output = edit_function(&output, "rubyLiveFrontReflection",
        "rubyResponse.rows[u32(responseAddress)+28u].xyz", "rubyResponse.rows[u32(responseAddress)+rubyFrontOffset(componentId)].xyz")?;
    let output = edit_function(&output, "rubyLiveEngraving",
        "    if(!rubyLiveTextOwner(hit.panel)||hit.distance<=0.0){return cachedRGB;}",
        "    if(rubyPixelEnabled(hit.panel)){return cachedRGB;}\n    if(!rubyLiveTextOwner(hit.panel)||hit.distance<=0.0){return cachedRGB;}")?;
    Ok(format!("{output}\n{support}\n{entry}\n{trace}\n"))
}

const LEAF_WRITE: &str = r#"            rubyResponse.rows[leaf]=rubyPixelPositions[lobe];
            rubyResponse.rows[leaf+1u]=vec4<f32>(rubyPixelDirections[lobe],0.0);
            rubyResponse.rows[leaf+2u]=vec4<f32>(rubyPixelWeights[lobe],0.0);"#;

const ENGRAVED_LEAF_WRITE: &str = r#"            rubyResponse.rows[leaf]=vec4<f32>(rubyPixelPositions[lobe].xyz,rubyPixelEmission[lobe].x);
            rubyResponse.rows[leaf+1u]=vec4<f32>(rubyPixelDirections[lobe],rubyPixelEmission[lobe].y);
            rubyResponse.rows[leaf+2u]=vec4<f32>(rubyPixelWeights[lobe],rubyPixelEmission[lobe].z);"#;

const ENGRAVED_RECORD: &str = r#"fn rubyPixelRecord(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32) {
    let slot=rubyPixelEvent;
    // Direct studio termination permits at most one exterior branch per
    // dielectric event. Any changed trace topology must use canonical tracing.
    if(slot<0||slot>=16){rubyPixelDropped+=weight;return;}
    if(gemMax(rubyPixelWeights[slot])>0.0){rubyPixelDropped+=weight;return;}
    rubyPixelPositions[slot]=vec4<f32>(origin,f32(depth));
    rubyPixelDirections[slot]=direction;
    rubyPixelWeights[slot]=weight;
}"#;

const SUPPORT: &str = r#"
// Pixel-mode transport keeps all chronological terminal branches. Original
// world coordinates and direction bits are retained, unlike the reusable
// object-grid cache, because every pose/camera change invalidates this mode.
var<private> rubyPixelTarget:i32;
var<private> rubyPixelPose:vec4<f32>;
var<private> rubyPixelHead:vec4<f32>;
var<private> rubyPixelDropped:vec3<f32>;
var<private> rubyPixelLeaves:i32;
var<private> rubyPixelPositions:array<vec4<f32>,16>;
var<private> rubyPixelDirections:array<vec3<f32>,16>;
var<private> rubyPixelWeights:array<vec3<f32>,16>;

fn rubyPixelEnabled(componentId:i32)->bool {
    let slot=rubyResponseSlot(componentId);
    if(slot<0){return false;}
    return rubyPlan.rows[1u+u32(slot)*8u+4u].w==1.0;
}

fn rubyResolvedOffset(componentId:i32)->u32 {
    if(rubyPixelEnabled(componentId)){return 51u;}
    return 27u;
}

fn rubyFrontOffset(componentId:i32)->u32 {
    if(rubyPixelEnabled(componentId)){return 52u;}
    return 28u;
}

fn rubyPixelEligible(at:u32)->bool {
    let descriptor=rubyPlan.rows[at];let shape=rubyPlan.rows[at+4u];
    let componentId=i32(descriptor.x);
    if(descriptor.x!=f32(componentId)){return false;}
    if(componentId>=700000&&componentId<704096){
        let instance=(componentId-700000)/16;
        if((componentId-700000)%16!=0||f32(instance)>=gemRow(248).w||f32(instance)>=gemRow(248).x){return false;}
        let timelineAt=i32(gemRow(248).y)+instance*3;
        if(gemRow(timelineAt).w<=0.0||gemRow(timelineAt+1).w>0.5){return false;}
        // CPU admission proves one material and an unclipped closed mesh.
    }else if(componentId<4+4*256||componentId>=4+10*256){return false;}
    if(descriptor.y!=64.0||shape.w!=1.0||shape.z!=108.0){return false;}
    if(!(shape.x>=1.0&&shape.y>=1.0&&shape.x*shape.y<=16384.0)){return false;}
    if(shape.x!=floor(shape.x)||shape.y!=floor(shape.y)){return false;}
    let responseRows=arrayLength(&rubyResponse.rows);
    let allocated=rubyPlan.rows[at+6u].x;
    if(!(descriptor.z>=0.0&&descriptor.z<=f32(responseRows))||descriptor.z!=floor(descriptor.z)){return false;}
    if(!(allocated>=1.0&&allocated<=f32(responseRows))||allocated!=floor(allocated)){return false;}
    let first=u32(descriptor.z);let rows=u32(allocated);
    if(rows>responseRows-first||shape.x*shape.y*108.0>allocated){return false;}
    if(gemRow(1).w!=0.0||gemRow(0).z!=2.0){return false;}
    if(any(gemRow(2).xyz!=vec3<f32>(1.0,0.0,0.0))||gemRow(2).w<=0.0){return false;}
    if(any(gemRow(3).xyz!=vec3<f32>(0.0,1.0,0.0))||gemRow(3).w<=0.0){return false;}
    if(any(gemRow(4).xyz!=vec3<f32>(0.0,0.0,-1.0))){return false;}
    let resolution=gemRow(8).xy;
    if(any(resolution<=vec2<f32>(0.0))||any(resolution!=floor(resolution))){return false;}
    if(gemRow(8).z<1.0||gemRow(8).z>16.0||gemRow(8).w<1.0||gemRow(8).w>64.0){return false;}
    let rectangle=rubyPlan.rows[at+5u].xy;
    if(any(rectangle<vec2<f32>(0.0))||any(rectangle!=floor(rectangle))){return false;}
    if(any(rectangle+shape.xy>resolution)){return false;}
    let base=gemPanelRow(componentId);let material=gemRow(base);
    if(material.w<=0.99||material.y<=1.0||material.z<0.0||material.z>0.1||gemRow(base+2).w!=0.0){return false;}
    if(rubyPlan.rows[at+1u].w<=0.0){return false;}
    return true;
}

fn rubyPixelRecord(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32) {
    let slot=rubyPixelLeaves;
    if(slot<16){
        rubyPixelPositions[slot]=vec4<f32>(origin,f32(depth));
        rubyPixelDirections[slot]=direction;
        rubyPixelWeights[slot]=weight;
    }else{rubyPixelDropped+=weight;}
    rubyPixelLeaves+=1;
}

fn rubyPixelTerminal(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32)->vec4<f32> {
    // Match the installed canonical studio policy, including concave controls:
    // every exterior branch immediately terminates without a self-entry test.
    rubyPixelRecord(origin,direction,weight,depth);
    return vec4<f32>(0.0,0.0,0.0,1.0);
}

fn rubyPixelComponentHit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,componentId:i32,farLimit:f32)->GemHit {
    if(componentId>=700000&&componentId<704096){
        // Single-material, non-tape handles retain the original triangle
        // order, clipping, normal and side tests for both entry and exit.
        return gemTimelineMesh(origin,ray,(componentId-700000)/16,medium,farLimit);
    }
    if(componentId<4+4*256||componentId>=4+10*256){return GemHit(farLimit,vec3<f32>(0.0),-1);}
    if(medium>=0){return gemControlExit(origin,ray,componentId,farLimit);}
    return rubyPixelControlEntry(origin,ray,-1,farLimit,false);
}

fn rubyPixelAddress(origin:vec3<f32>,componentId:i32,random:vec2<f32>,at:u32)->i32 {
    if(!rubyPixelEligible(at)){return -1;}
    let descriptor=rubyPlan.rows[at];let shape=rubyPlan.rows[at+4u];
    let rectangle=vec2<i32>(rubyPlan.rows[at+5u].xy);
    var sample:i32=0;if(random.x>0.5){sample=1;}
    let xi=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sample));
    let camera=gemRow(1);let right=gemRow(2);let up=gemRow(3);let resolution=gemRow(8).xy;
    let x=i32(round(((origin.x-camera.x)/right.w+1.0)*resolution.x/2.0-xi.x));
    let y=i32(round((1.0-(origin.y-camera.y)/up.w)*resolution.y/2.0-1.0+xi.y));
    let xy=vec2<i32>(x,y)-rectangle;
    if(any(xy<vec2<i32>(0))||any(xy>=vec2<i32>(shape.xy))){return -1;}
    let address=i32(descriptor.z)+(xy.y*i32(shape.x)+xy.x)*108+sample*54;
    if(address<0||u32(address)+53u>=arrayLength(&rubyResponse.rows)){return -1;}
    if(rubyResponse.rows[u32(address)].w<0.5){return -1;}
    if(rubyResponse.rows[u32(address)+51u].w<0.5||rubyResponse.rows[u32(address)+52u].w<0.5){return -1;}
    return address;
}

fn rubyPixelBake(id:vec3<u32>,at:u32) {
    let descriptor=rubyPlan.rows[at];
    if(descriptor.w!=2.0||!rubyPixelEligible(at)){return;}
    let shape=vec2<u32>(rubyPlan.rows[at+4u].xy);
    let index=id.y*128u+id.x;
    if(id.x>=128u||id.y>=128u||index>=shape.x*shape.y){return;}
    let xy=vec2<u32>(rubyPlan.rows[at+5u].xy)+vec2<u32>(index%shape.x,index/shape.x);
    let first=u32(descriptor.z)+index*108u;
    if(first+107u>=arrayLength(&rubyResponse.rows)){return;}
    rubyPixelTarget=i32(descriptor.x);rubyPixelPose=rubyPlan.rows[at+1u];
    let screen=vec2<f32>((f32(xy.x)+0.5)*2.0/gemRow(8).x-1.0,1.0-(f32(xy.y)+0.5)*2.0/gemRow(8).y);
    var camera:vec4<f32>=gemRow(1);var right:vec4<f32>=gemRow(2);var up:vec4<f32>=gemRow(3);var forward:vec4<f32>=gemRow(4);
    var resolution:vec2<f32>=max(gemRow(8).xy,vec2<f32>(1.0));
    for(var sample:u32=0u;sample<2u;sample++){
        rubyPixelHead=vec4<f32>(0.0);rubyPixelDropped=vec3<f32>(0.0);rubyPixelLeaves=0;
        for(var lobe:i32=0;lobe<16;lobe++){
            rubyPixelPositions[lobe]=vec4<f32>(0.0);
            rubyPixelDirections[lobe]=vec3<f32>(0.0);rubyPixelWeights[lobe]=vec3<f32>(0.0);
        }
        var xi:vec2<f32>=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sample));
        var pixel:vec2<f32>=screen+(xi-vec2<f32>(0.5))*2.0/resolution;
        var offset:vec3<f32>=right.xyz*(pixel.x*right.w)+up.xyz*(pixel.y*up.w);
        var origin:vec3<f32>=camera.xyz+offset;var direction:vec3<f32>=gemSafeUnit(forward.xyz);
        var random:vec2<f32>=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sample));
        let radiance=rubyPixelTrace(origin,direction,random);
        if(rubyPixelLeaves>16||any(rubyPixelDropped!=vec3<f32>(0.0))){rubyPixelHead.w=0.0;}
        let address=first+sample*54u;
        rubyResponse.rows[address]=rubyPixelHead;
        rubyResponse.rows[address+1u]=vec4<f32>(rubyPixelDropped,f32(rubyPixelLeaves));
        for(var lobe:u32=0u;lobe<16u;lobe++){
            let leaf=address+2u+3u*lobe;
            rubyResponse.rows[leaf]=rubyPixelPositions[lobe];
            rubyResponse.rows[leaf+1u]=vec4<f32>(rubyPixelDirections[lobe],0.0);
            rubyResponse.rows[leaf+2u]=vec4<f32>(rubyPixelWeights[lobe],0.0);
        }
        rubyResponse.rows[address+50u]=vec4<f32>(radiance,0.0);
        rubyResponse.rows[address+51u]=vec4<f32>(0.0);
        rubyResponse.rows[address+52u]=vec4<f32>(0.0);
        rubyResponse.rows[address+53u]=vec4<f32>(1.0,rubyPixelHead.w,f32(rubyPixelLeaves),gemMax(rubyPixelDropped));
    }
}

fn rubyPixelResolve(id:vec3<u32>,at:u32) {
    let descriptor=rubyPlan.rows[at];
    if(descriptor.w<0.5||!rubyPixelEligible(at)){return;}
    let shape=vec2<u32>(rubyPlan.rows[at+4u].xy);
    let index=id.y*128u+id.x;
    if(id.x>=128u||id.y>=128u||index>=shape.x*shape.y){return;}
    let first=u32(descriptor.z)+index*108u;
    if(first+107u>=arrayLength(&rubyResponse.rows)){return;}
    for(var sample:u32=0u;sample<2u;sample++){
        let address=first+sample*54u;
        rubyResponse.rows[address+51u]=vec4<f32>(0.0);
        rubyResponse.rows[address+52u]=vec4<f32>(0.0);
        if(rubyResponse.rows[address].w<0.5){continue;}
        var result=vec3<f32>(0.0);var front=vec3<f32>(0.0);
        result+=rubyResponse.rows[address+50u].xyz;
        let count=u32(rubyResponse.rows[address+1u].w);
        if(count>16u){continue;}
        for(var lobe:u32=0u;lobe<16u;lobe++){
            if(lobe>=count){break;}
            let leaf=address+2u+3u*lobe;
            let weight=rubyResponse.rows[leaf+2u].xyz;
            let position=rubyResponse.rows[leaf];
            let exitOrigin=position.xyz;
            let exitDirection=rubyResponse.rows[leaf+1u].xyz;
            let terminal=gemTerminal(exitOrigin,exitDirection);
            result+=weight*terminal.xyz;
            if(i32(position.w)==1){front+=weight*terminal.xyz;}
        }
        rubyResponse.rows[address+51u]=vec4<f32>(result,1.0);
        rubyResponse.rows[address+52u]=vec4<f32>(front,1.0);
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROL: &str = r#"            if(inside&&current.medium>=4){
                var engraving:vec4<f32>=gemControlEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
                var navigation:vec4<f32>=gemPluginNavigationEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*navigation.xyz;current.weight*=navigation.w;
            }
"#;

    const TIMELINE: &str = r#"            if(inside&&current.medium>=700000&&current.medium<704096){
                var engraving:vec4<f32>=gemTimelineEngraving(current.origin,current.direction,current.medium,hit.distance);
                result+=current.weight*engraving.xyz;current.weight*=engraving.w;
            }
"#;

    fn fixture() -> (String, String) {
        let original = format!("fn rubyTraceFallback() {{\n{CONTROL}{TIMELINE}{BEER}}}\n\
            fn gemControlEngraving() {{{CONTROL_GUARD}}}\n\
            fn gemPluginNavigationEngraving() {{{NAVIGATION_GUARD}}}\n\
            fn gemTimelineEngraving() {{{TIMELINE_GUARD}}}\n");
        let trace = format!("fn rubyPixelTrace() {{\n        if(f32(event)>=gemRow(8).w){{break;}}\n{BEER}}}\n");
        (original, trace)
    }

    #[test]
    fn workstation_pixel_engraving_precedes_segment_beer_and_keeps_event_sum_order() {
        let (original, trace) = fixture();
        let (trace, support) = full_engraving(&original, &trace, SUPPORT).unwrap();
        let contribution = trace.find("result+=engravedContribution;").unwrap();
        let opacity = trace.find("current.weight*=engraving.w;").unwrap();
        let timeline = trace.find("let timelineContribution=current.weight*engraving.xyz;").unwrap();
        let timeline_sum = trace.find("result+=timelineContribution;current.weight*=engraving.w;").unwrap();
        let beer = trace.find(BEER).unwrap();
        assert!(contribution < opacity && opacity < timeline && timeline < timeline_sum && timeline_sum < beer);
        assert_eq!(trace.matches("gemTimelineEngraving(current.origin,current.direction,current.medium,hit.distance)").count(), 1);
        assert!(trace.contains("rubyPixelEmission[rubyPixelEvent]=timelineContribution;"));
        let resolve = function(&support, "rubyPixelResolve").unwrap();
        assert!(resolve.find("result+=emission;").unwrap() < resolve.find("result+=weight*terminal.xyz;").unwrap());
        assert!(!resolve.contains("result+=rubyResponse.rows[address+50u]"));
        assert!(trace.contains("rubyPixelEvent=event;rubyPixelLeaves=event+1;"));
        let swapped = original.replace(&format!("{TIMELINE}{BEER}"), &format!("{BEER}{TIMELINE}"));
        assert!(full_engraving(&swapped, &trace, SUPPORT).is_err());
        let reversed = original.replace(&format!("{CONTROL}{TIMELINE}"), &format!("{TIMELINE}{CONTROL}"));
        assert!(full_engraving(&reversed, &trace, SUPPORT).is_err());
    }

    #[test]
    fn workstation_pixel_guarded_ids_capacity_and_overflow_retain_canonical_fallback() {
        let (original, trace) = fixture();
        let (trace, support) = full_engraving(&original, &trace, SUPPORT).unwrap();
        let eligible = function(&support, "rubyPixelEligible").unwrap();
        for guard in ["componentId<4+4*256||componentId>=4+10*256",
                      "componentId>=700000&&componentId<704096", "(componentId-700000)%16!=0",
                      "f32(instance)>=gemRow(248).w", "gemRow(timelineAt+1).w>0.5",
                      "shape.x*shape.y<=16384.0", "let allocated=rubyPlan.rows[at+6u].x;",
                      "rows>responseRows-first||shape.x*shape.y*108.0>allocated",
                      "descriptor.z!=floor(descriptor.z)", "allocated!=floor(allocated)",
                      "gemRow(8).z>16.0", "gemRow(8).w>64.0",
                      "any(rectangle+shape.xy>resolution)"] {
            assert!(eligible.contains(guard), "missing pixel admission guard: {guard}");
        }
        assert!(trace.contains("if(event>=16){rubyPixelHead.w=0.0;rubyPixelDropped=vec3<f32>(1.0);return vec3<f32>(0.0);}"));
        assert!(support.contains("if(slot<0||slot>=16){rubyPixelDropped+=weight;return;}"));
        assert!(support.contains("if(gemMax(rubyPixelWeights[slot])>0.0){rubyPixelDropped+=weight;return;}"));
        assert!(support.contains("if(rubyPixelLeaves>16||any(rubyPixelDropped!=vec3<f32>(0.0))){rubyPixelHead.w=0.0;}"));
        assert!(function(&support, "rubyPixelAddress").unwrap().contains("rubyResponse.rows[u32(address)].w<0.5){return -1;}"));
    }

    #[test]
    fn workstation_pixel_rejects_changed_marker_navigation_and_malformed_source() {
        let (original, trace) = fixture();
        for guard in [CONTROL_GUARD, NAVIGATION_GUARD, TIMELINE_GUARD] {
            assert!(full_engraving(&original.replace(guard, "return vec4<f32>(0.0);"), &trace, SUPPORT).is_err());
        }
        assert!(full_engraving(&original, &trace.replace(BEER, ""), SUPPORT).is_err());
        assert!(source("invalid WGSL").is_err());
        assert!(source("fn rubyPixelAlreadyApplied() {}").is_err());
        assert!(closing("{", 0).is_err());
    }

    #[test]
    fn workstation_pixel_timeline_slot_mapping_is_opt_in_and_bounds_checked_before_read() {
        let slot = format!(r#"fn rubyResponseSlot(componentId:i32)->i32 {{
    // Timeline IDs have a reserved map range but currently keep canonical
    // tracing: their component-only entry traversal has not been implemented.
{LEGACY_SLOT_GUARD}
    if(rubyPlan.rows[0].y<0.5){{return -1;}}
    let slot=i32(rubyPlan.rows[161u+u32(componentId)/4u][u32(componentId)%4u]);
    if(slot<0||slot>=20||f32(slot)>=rubyPlan.rows[0].x){{return -1;}}
    let descriptor=rubyPlan.rows[1u+u32(slot)*8u];
    if(i32(descriptor.x)!=componentId||descriptor.w<0.5){{return -1;}}
    return slot;
}}"#);
        let mapped = timeline_slots(&slot).unwrap();
        assert!(mapped.contains("componentId>=700000&&componentId<704096"));
        assert!(mapped.contains("mapping=16644+(componentId-700000);"));
        assert!(mapped.contains("else if(componentId>=16644){return -1;}"));
        assert!(mapped.find("if(mapRow>=arrayLength(&rubyPlan.rows)){return -1;}").unwrap()
            < mapped.find("rubyPlan.rows[mapRow][u32(mapping)%4u]").unwrap());
        assert!(mapped.contains("if(timeline&&rubyPlan.rows[1u+u32(slot)*8u+4u].w!=1.0){return -1;}"));
        assert!(timeline_slots(&mapped).is_err());
        assert!(timeline_slots(&slot.replace("componentId>=16644", "componentId>=700000")).is_err());
    }

    #[test]
    fn workstation_pixel_timeline_keeps_original_mesh_entry_and_exit() {
        let component = function(SUPPORT, "rubyPixelComponentHit").unwrap();
        let timeline = component.find("return gemTimelineMesh(origin,ray,(componentId-700000)/16,medium,farLimit);").unwrap();
        assert!(timeline < component.find("return gemControlExit(").unwrap());
        assert!(!component.contains("rubyControlEntry("));
    }
}
