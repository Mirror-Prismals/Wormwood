#!/usr/bin/env python3
"""Diagnostic exact screen-pixel transport layered on guarded-convex ruby WGSL.

This module does not change production sources. ``transform(source)`` accepts
the exported production shader and returns a shader with an additional cache
mode selected by descriptor row 4.w == 1. The CPU descriptor ABI is documented
in /tmp/ruby-pixel-cache-contract.txt. The new mode stores original WORLD exit
origins and unmodified directions: translation invalidates it, so the legacy
local-coordinate round trip and direction normalization are unnecessary and
would perturb reflections. Legacy cache modes retain their original behavior.
"""

from pathlib import Path
import argparse


def once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError(f"pixel cache marker changed ({source.count(old)}): {old}")
    return source.replace(old, new, 1)


def braced_end(source: str, opening: int) -> int:
    if source[opening] != "{":
        raise ValueError("expected opening brace")
    depth = 1
    end = opening + 1
    while depth and end < len(source):
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    if depth:
        raise ValueError("pixel cache block is unclosed")
    return end


def function_span(source: str, name: str) -> tuple[int, int]:
    marker = f"fn {name}("
    if source.count(marker) != 1:
        raise ValueError(f"pixel cache function changed: {name}")
    start = source.index(marker)
    opening = source.index("{", start)
    return start, braced_end(source, opening)


def function(source: str, name: str) -> str:
    start, end = function_span(source, name)
    return source[start:end]


def edit_function(source: str, name: str, old: str, new: str) -> str:
    start, end = function_span(source, name)
    return source[:start] + once(source[start:end], old, new) + source[end:]


SUPPORT = r"""
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
    if(componentId<4+4*256||componentId>=4+10*256){return false;}
    if(descriptor.y!=64.0||shape.w!=1.0||shape.z!=108.0){return false;}
    if(shape.x<1.0||shape.y<1.0||shape.x!=floor(shape.x)||shape.y!=floor(shape.y)){return false;}
    if(shape.x*shape.y>2275.0){return false;}
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
"""


ENGRAVED_RECORD = r"""fn rubyPixelRecord(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32) {
    let slot=rubyPixelEvent;
    // Direct studio termination permits at most one exterior branch per
    // dielectric event. Any changed trace topology must use canonical tracing.
    if(slot<0||slot>=16){rubyPixelDropped+=weight;return;}
    if(gemMax(rubyPixelWeights[slot])>0.0){rubyPixelDropped+=weight;return;}
    rubyPixelPositions[slot]=vec4<f32>(origin,f32(depth));
    rubyPixelDirections[slot]=direction;
    rubyPixelWeights[slot]=weight;
}"""


def full_engraving(source: str, trace: str, support: str) -> tuple[str, str]:
    """Preserve all interior engraving and the canonical event sum order.

    The caller's CPU key MUST include engraving density/parameters/lighting.
    This optional variant uses spare leaf .w lanes for each event's engraved
    RGB, so a reflection-only/TIR event needs no additional storage. Positions,
    directions, weights and event count otherwise retain the descriptor ABI.
    """
    fallback = function(source, "rubyTraceFallback")
    marker = "            if(inside&&current.medium>=4){"
    if fallback.count(marker) != 1:
        raise ValueError("canonical control engraving branch changed")
    start = fallback.index(marker)
    end = braced_end(fallback, fallback.index("{", start))
    engraving = fallback[start:end] + "\n"
    engraving = once(engraving,
                     "                result+=current.weight*engraving.xyz;current.weight*=engraving.w;",
                     "                let engravedContribution=current.weight*engraving.xyz;\n"
                     "                rubyPixelEmission[rubyPixelEvent]=engravedContribution;\n"
                     "                result+=engravedContribution;current.weight*=engraving.w;")
    # Navigation is identity for the admitted original instances4..9; retain
    # its original call and arithmetic while asserting that precondition.
    navigation = function(source, "gemPluginNavigationEngraving")
    if "if(instance<61||instance>=65||abs(ray.z)<0.00001){return vec4<f32>(0.0,0.0,0.0,1.0);}" not in navigation:
        raise ValueError("navigation engraving ownership changed")
    marker = "            if(inside){current.weight*=exp(-gemRow(base+1).xyz*hit.distance/gemObjectScale(hit.panel));}\n"
    if fallback.count(marker) != 1 or fallback.index(marker) <= end:
        raise ValueError("canonical engraving/Beer operation order changed")
    # Engraving integrates its own attenuation only as far as each text layer.
    # The original full-segment Beer update occurs AFTER engraved radiance and
    # text opacity; moving it ahead would double-attenuate the text contribution.
    trace = once(trace, marker, engraving + marker)
    marker = "        if(f32(event)>=gemRow(8).w){break;}\n"
    trace = once(trace, marker, marker +
                 "        if(event>=16){rubyPixelHead.w=0.0;rubyPixelDropped=vec3<f32>(1.0);return vec3<f32>(0.0);}\n"
                 "        rubyPixelEvent=event;rubyPixelLeaves=event+1;\n")
    support = once(support,
                   "var<private> rubyPixelLeaves:i32;",
                   "var<private> rubyPixelLeaves:i32;\n"
                   "var<private> rubyPixelEvent:i32;\n"
                   "var<private> rubyPixelEmission:array<vec3<f32>,16>;")
    support = once(support,
                   "||gemRow(base+2).w!=0.0){return false;}",
                   "||gemRow(base+2).w<0.0){return false;}")
    start, end = function_span(support, "rubyPixelRecord")
    support = support[:start] + ENGRAVED_RECORD + support[end:]
    support = once(support,
                   "            rubyPixelPositions[lobe]=vec4<f32>(0.0);",
                   "            rubyPixelPositions[lobe]=vec4<f32>(0.0);\n"
                   "            rubyPixelEmission[lobe]=vec3<f32>(0.0);")
    support = once(support,
                   "            rubyResponse.rows[leaf]=rubyPixelPositions[lobe];\n"
                   "            rubyResponse.rows[leaf+1u]=vec4<f32>(rubyPixelDirections[lobe],0.0);\n"
                   "            rubyResponse.rows[leaf+2u]=vec4<f32>(rubyPixelWeights[lobe],0.0);",
                   "            rubyResponse.rows[leaf]=vec4<f32>(rubyPixelPositions[lobe].xyz,rubyPixelEmission[lobe].x);\n"
                   "            rubyResponse.rows[leaf+1u]=vec4<f32>(rubyPixelDirections[lobe],rubyPixelEmission[lobe].y);\n"
                   "            rubyResponse.rows[leaf+2u]=vec4<f32>(rubyPixelWeights[lobe],rubyPixelEmission[lobe].z);")
    support = once(support, "        result+=rubyResponse.rows[address+50u].xyz;\n",
                   "        // Intrinsic row50 is diagnostic: event RGB is accumulated below\n"
                   "        // in canonical order, interleaved with each terminal contribution.\n")
    support = once(support,
                   "            let weight=rubyResponse.rows[leaf+2u].xyz;",
                   "            let weight=rubyResponse.rows[leaf+2u].xyz;\n"
                   "            let emission=vec3<f32>(rubyResponse.rows[leaf].w,rubyResponse.rows[leaf+1u].w,rubyResponse.rows[leaf+2u].w);\n"
                   "            result+=emission;\n"
                   "            if(gemMax(weight)<=0.0){continue;}")
    support = once(support,
                   "            if(i32(position.w)==1){front+=weight*terminal.xyz;}",
                   "            if(lobe==0u){front+=weight*terminal.xyz;}")
    return trace, support


def transform(source: str, *, engraved: bool = False) -> str:
    """Return diagnostic WGSL, rejecting unexpected or already patched input."""
    if len(source) > 2 * 1024 * 1024 or "fn rubyPixel" in source:
        raise ValueError("unexpected pixel-cache input")
    # Guard the installed exterior policy. New modes may not reinterpret any
    # geometry hit, material, sample, or bounded-integrator arithmetic.
    terminal = function(source, "gemTerminal")
    for marker in ("let environment=gemEnvironment(origin,ray);",
                   "gemFloor(origin+ray*environment.w,-ray)",
                   "return vec4<f32>(radiance,1.0);"):
        if terminal.count(marker) != 1:
            raise ValueError("pixel cache requires the installed studio policy")
    if "gemNearest(" in terminal or "gemAny(" in terminal:
        raise ValueError("pixel cache requires direct studio termination")
    trace = function(source, "rubyBakeTrace")
    for marker in ("gemEngraving(", "gemControlEngraving(",
                   "gemPluginNavigationEngraving(", "gemTimelineEngraving(",
                   "rubyResponseAddress(", "gemTerminal("):
        if marker in trace:
            raise ValueError(f"unexpected intrinsic trace operation: {marker}")
    if trace.count("rubyBakeTerminal(") != 4 or trace.count("rubyRecord(") != 1:
        raise ValueError("intrinsic terminal branch structure changed")
    # Copy the installed integrator verbatim, only rebinding its intrinsic
    # storage and terminal callbacks. Sampling/Fresnel/Beer/GGX/order are shared.
    trace = once(trace, "fn rubyBakeTrace(", "fn rubyPixelTrace(")
    trace = once(trace,
                 "rubyComponentHit(current.origin,current.direction,current.medium,rubyBakeTarget,1e20)",
                 "rubyPixelComponentHit(current.origin,current.direction,current.medium,rubyBakeTarget,1e20)")
    for old, new in (("rubyBakeTerminal", "rubyPixelTerminal"),
                     ("rubyRecord", "rubyPixelRecord"),
                     ("rubyBakeTarget", "rubyPixelTarget"),
                     ("rubyBakePose", "rubyPixelPose"),
                     ("rubyBakeHead", "rubyPixelHead")):
        trace = trace.replace(old, new)
    # Primary controls always use authored mesh triangles in the canonical
    # renderer, including clear windows whose interior uses convex planes.
    # Retain its original full-instance BVH and triangle ordering, filtering
    # only irrelevant instances/materials. The legacy cache's component entry
    # is intentionally not reused: it would use different plane entry math.
    entry = function(source, "gemControlHit")
    entry = once(entry, "fn gemControlHit(", "fn rubyPixelControlEntry(")
    entry = once(entry,
                 "        var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);",
                 "        if(instance!=(rubyPixelTarget-4)/256){continue;}\n"
                 "        var monitor:bool=(instance>=10&&instance<13)||(instance>=29&&instance<61);")
    entry = once(entry,
                 "                var material:i32=4+instance*256+i32(a.w);",
                 "                var material:i32=4+instance*256+i32(a.w);\n"
                 "                if(material!=rubyPixelTarget){continue;}")
    support = SUPPORT
    if engraved:
        trace, support = full_engraving(source, trace, support)
    # Insert new dispatches before legacy 64x64 bounds checks. Their flattened
    # index reuses the existing 128x128 dispatch and each control's fixed slab.
    descriptor = "    let at=1u+id.z*8u;let descriptor=rubyPlan.rows[at];\n"
    source = edit_function(source, "cs_ruby_response_general", descriptor,
                           descriptor + "    if(rubyPlan.rows[at+4u].w==1.0){rubyPixelBake(id,at);return;}\n")
    source = edit_function(source, "cs_ruby_resolve", descriptor,
                           descriptor + "    if(rubyPlan.rows[at+4u].w==1.0){rubyPixelResolve(id,at);return;}\n")
    descriptor = "    let at=1u+u32(slot)*8u;let descriptor=rubyPlan.rows[at];\n"
    source = edit_function(source, "rubyResponseAddress", descriptor,
                           descriptor + "    if(rubyPlan.rows[at+4u].w==1.0){return rubyPixelAddress(origin,componentId,random,at);}\n")
    source = edit_function(source, "rubyTraceFallback",
                           "rubyResponse.rows[address+27u].xyz",
                           "rubyResponse.rows[address+rubyResolvedOffset(hit.panel)].xyz")
    source = edit_function(source, "rubyFastTrace",
                           "rubyResponse.rows[u32(responseAddress)+27u].xyz",
                           "rubyResponse.rows[u32(responseAddress)+rubyResolvedOffset(hit.panel)].xyz")
    source = edit_function(source, "rubyLiveFrontReflection",
                           "rubyResponse.rows[u32(responseAddress)+28u].xyz",
                           "rubyResponse.rows[u32(responseAddress)+rubyFrontOffset(componentId)].xyz")
    source = edit_function(source, "rubyLiveEngraving",
                           "    if(!rubyLiveTextOwner(hit.panel)||hit.distance<=0.0){return cachedRGB;}",
                           "    if(rubyPixelEnabled(hit.panel)){return cachedRGB;}\n"
                           "    if(!rubyLiveTextOwner(hit.panel)||hit.distance<=0.0){return cachedRGB;}")
    return source + "\n" + support + "\n" + entry + "\n" + trace + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--engraved", action="store_true",
                        help="include all engraving segments; requires corresponding CPU invalidation key")
    args = parser.parse_args()
    output = transform(args.input.read_text(), engraved=args.engraved)
    with args.output.open("x") as stream:
        stream.write(output)


if __name__ == "__main__":
    main()
