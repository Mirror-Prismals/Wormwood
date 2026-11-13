"""Diagnostic queue-free intrinsic bake for every planned native ruby component.

transform(production_source) applies the existing guarded convex bake and panel
escape certificate, then extends speculation to component IDs 0..16643. Panels
retain their exact entry/exit functions plus the escape certificate. Controls
retain rubyComponentHit and rubyBakeTerminal, including their original triangle
BVH order, material filtering, authored normals, tolerances and local transform.
No control geometry is converted to planes or assumed to be convex.

Every accepted sample has one continuing interior ray and event == depth. A
terminal self-reentry, two continuing branches, unexpected medium/component or
opaque material rejects the sample. The original kernel reruns from the same
origin and seed after all bake accumulators are reset. Copied dielectric math,
cutoffs, budgets and leaf order remain unchanged. Convexity affects acceptance
rate, not the correctness of this speculative path.

split(specialized_source) separates the queue-free kernel from its canonical
fallback into cs_ruby_response and cs_ruby_response_general. The host MUST
dispatch both in that order in separate passes, with the same dirty plan and
dimensions, before resolve/playback. Only row29.z-flagged samples run general.
transform_split(production_source) is the convenience composition.

Row29 for EVERY cached sample contains [attempted, accepted, fallback, events].
Events count speculative work only. This conflicts with the separate floor
cache diagnostic's row29 use. No application loads this module; no ABI changes.
"""

import ruby_convex_bake as convex
import ruby_convex_escape as escape


_MARKER = "// RUBY_SINGLE_BRANCH_ALL_NATIVE_COMPONENTS\n"
_DISPATCH = convex._DISPATCH.replace(
    "rubyBakeTarget>=0&&rubyBakeTarget<4",
    "rubyBakeTarget>=0&&rubyBakeTarget<16644",
)

_SPECULATIVE_DISPATCH = """        rubyConvexStats=vec4<f32>(1.0,0.0,0.0,0.0);
        rubyConvexUnsupported=false;
        let radiance=rubyConvexBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        if(rubyConvexUnsupported){rubyConvexStats.z=1.0;}
        else{rubyConvexStats.y=1.0;}"""

_GENERAL_SAMPLE = """    for(var sample:u32=0u;sample<2u;sample++){
        let generalAddress=first+sample*30u;
        let savedStats=rubyResponse.rows[generalAddress+29u];
        if(savedStats.z<0.5){continue;}
"""

_GENERAL_DISPATCH = """        let radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        rubyConvexStats=savedStats;"""


def transform(source: str) -> str:
    """Extend production's guarded panel specialization to all native targets."""
    if _MARKER in source or "fn rubyConvexBakeTrace(" in source:
        raise ValueError("Single-branch transform requires original production source")
    # Keep the existing copied dielectric equations, guards and fallback reset.
    source = escape.transform(convex.transform(source))
    kernel = convex._function(source, "rubyConvexBakeTrace")
    changed = convex._once(kernel,
        "        if(current.medium>=0){hit=rubyConvexExitPanel(current.origin,current.direction,rubyBakeTarget);}\n"
        "        else{hit=rubyConvexEnterPanel(current.origin,current.direction,gemInverseRay(current.direction),rubyBakeTarget,1e20);}",
        "        if(rubyBakeTarget<4){\n"
        "            if(current.medium>=0){hit=rubyConvexExitPanel(current.origin,current.direction,rubyBakeTarget);}\n"
        "            else{hit=rubyConvexEnterPanel(current.origin,current.direction,gemInverseRay(current.direction),rubyBakeTarget,1e20);}\n"
        "        }else{\n"
        "            hit=rubyComponentHit(current.origin,current.direction,current.medium,rubyBakeTarget,1e20);\n"
        "        }")
    changed = convex._once(changed,
        "hit.panel!=rubyBakeTarget||hit.panel<0||hit.panel>=4",
        "hit.panel!=rubyBakeTarget||hit.panel<0||hit.panel>=16644")
    source = convex._once(source, kernel, changed)

    # This early return must precede the panel certificate: a control triangle's
    # shading normal is not a supporting plane, and rubyConvexPlane is panel-only.
    terminal = convex._function(source, "rubyConvexTerminal")
    opening = terminal.index("{") + 1
    changed = terminal[:opening] + """
    if(rubyBakeTarget>=4){
        return rubyBakeTerminal(origin,direction,weight,depth);
    }
""" + terminal[opening:]
    source = convex._once(source, terminal, changed)

    compute = convex._function(source, "cs_ruby_response")
    changed = convex._once(compute, convex._DISPATCH, _DISPATCH)
    source = convex._once(source, compute, changed)
    source = convex._once(source,
        "// For a convex panel, every retained exterior ray terminates immediately.\n"
        "// Only its refracted entry or subsequent internal reflection can continue.\n"
        "// This preserves event==depth; a violation restarts with the general kernel.",
        "// Speculate one interior continuation per event for native components.\n"
        "// Control terminals retain their exact component self-hit traversal.\n"
        "// Reentry or any queue requirement restarts with the general kernel.")
    source = convex._once(source,
        "// The original component terminal query, specialized only by known panel ID.\n"
        "// Keep the full authored entry test until a separate escape certificate is\n"
        "// proven. No control BVH is reachable from this diagnostic panel kernel.",
        "// Controls retain their canonical terminal query. Panels use their exact\n"
        "// authored halfspace certificate and unchanged entry-test fallback.")
    return _MARKER + source


def split(source: str) -> str:
    """Split already-generalized speculation and canonical fallback pipelines."""
    if _MARKER not in source:
        raise ValueError("Single-branch split requires this module's transform first")
    if "fn cs_ruby_response_general(" in source:
        raise ValueError("Single-branch bake is already split")
    compute = convex._function(source, "cs_ruby_response")
    speculative = convex._once(compute, _DISPATCH, _SPECULATIVE_DISPATCH)
    if "rubyBakeTrace(" in speculative or "rubyConvexResetBake(" in speculative:
        raise ValueError("Speculative entry still calls the canonical kernel")
    if "if(rubyBakeTarget<0||rubyBakeTarget>=16644){return;}" not in speculative:
        raise ValueError("Native component dispatch range changed")

    general = convex._once(compute, "fn cs_ruby_response(", "fn cs_ruby_response_general(")
    general = convex._once(general,
        "    for(var sample:u32=0u;sample<2u;sample++){\n", _GENERAL_SAMPLE)
    general = convex._once(general, _DISPATCH, _GENERAL_DISPATCH)
    if "rubyConvexBakeTrace(" in general:
        raise ValueError("General entry must not repeat the speculative kernel")
    # The original reset and every cache-row write remain in each sample body.
    # Saved stats are read before reset and restored after the canonical rerun.
    return convex._once(source, compute, speculative) + \
        "\n@compute @workgroup_size(8,8,1)\n" + general + "\n"


def transform_split(source: str) -> str:
    """Transform original production source into the two-pass diagnostic."""
    return split(transform(source))


def statistics(source: str) -> str:
    """Map all 20 slots' row29 counters to a 640x480 diagnostic draw.

    Four panel slots reserve 32768 pixels each (two samples at 128x128).
    Sixteen control slots reserve 8192 each (two samples at 64x64). Unused
    descriptors and the remaining image pixels are zero. Never use for timing.
    """
    if _MARKER not in source:
        raise ValueError("Single-branch statistics requires this module's transform")
    source = convex._once(source, "fn workstationShade(", "fn workstationShadeOriginal(")
    return source + r'''
fn workstationShade(screen:vec2<f32>)->vec4<f32> {
    let x=u32(round((screen.x+1.0)*320.0-0.5));
    let y=u32(round((1.0-screen.y)*240.0-0.5));
    let index=y*640u+x;
    var slot=index/32768u;
    var item=index%32768u;
    if(index>=131072u){slot=4u+(index-131072u)/8192u;item=(index-131072u)%8192u;}
    if(arrayLength(&rubyPlan.rows)<161u||slot>=20u||f32(slot)>=rubyPlan.rows[0].x){return vec4<f32>(0.0);}
    let desc=rubyPlan.rows[1u+slot*8u];
    if(desc.w<0.5||(desc.y!=64.0&&desc.y!=128.0)||item>=u32(desc.y)*u32(desc.y)*2u){return vec4<f32>(0.0);}
    let address=u32(desc.z)+(item/2u)*60u+(item%2u)*30u+29u;
    if(address>=arrayLength(&rubyResponse.rows)){return vec4<f32>(0.0);}
    return rubyResponse.rows[address];
}
'''


source = transform
