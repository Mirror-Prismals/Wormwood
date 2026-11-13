"""Diagnostic separate panel and general intrinsic-bake compute pipelines.

Apply after ruby_convex_bake.transform and, optionally, ruby_convex_escape.
The first pipeline contains only the single-continuation panel kernel. It
stores speculative rows and marks rejected samples in the unused row29.z.
A separate compute pass then runs the original kernel for controls and those
rejected panel samples, resetting every accumulator and replacing every row.

The host must dispatch cs_ruby_response, then cs_ruby_response_general in a
separate pass, before cs_ruby_resolve or any cache playback. Both bake passes
use the dirty plan and the same dispatch dimensions. No new binding or cache
row is introduced. Row29 retains the original attempt counters on fallback.
This is diagnostic only; production never imports this module.
"""

from ruby_convex_bake import _DISPATCH, _function, _once


_PANEL_DISPATCH = """        rubyConvexStats=vec4<f32>(1.0,0.0,0.0,0.0);
        rubyConvexUnsupported=false;
        let radiance=rubyConvexBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        if(rubyConvexUnsupported){rubyConvexStats.z=1.0;}
        else{rubyConvexStats.y=1.0;}"""

_GENERAL_SAMPLE = """    for(var sample:u32=0u;sample<2u;sample++){
        let generalAddress=first+sample*30u;
        var savedStats:vec4<f32>=vec4<f32>(0.0);
        if(rubyBakeTarget<4){
            savedStats=rubyResponse.rows[generalAddress+29u];
            if(savedStats.z<0.5){continue;}
        }
"""

_GENERAL_DISPATCH = """        let radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        rubyConvexStats=savedStats;"""


def transform(source: str) -> str:
    """Split an already-specialized bake without changing any optical math."""
    if "fn cs_ruby_response_general(" in source:
        raise ValueError("Split convex ruby bake has already been applied")
    if "fn rubyConvexBakeTrace(" not in source:
        raise ValueError("Split bake requires ruby_convex_bake.transform first")
    compute = _function(source, "cs_ruby_response")
    panel = _once(compute, _DISPATCH, _PANEL_DISPATCH)
    panel = _once(panel,
        "    if(rubyBakeTarget<0||rubyBakeTarget>=16644){return;}",
        "    if(rubyBakeTarget<0||rubyBakeTarget>=4){return;}")
    if "rubyBakeTrace(" in panel or "rubyConvexResetBake(" in panel:
        raise ValueError("Panel entry still calls the general bake")

    general = _once(compute, "fn cs_ruby_response(", "fn cs_ruby_response_general(")
    general = _once(general,
        "    for(var sample:u32=0u;sample<2u;sample++){\n", _GENERAL_SAMPLE)
    general = _once(general, _DISPATCH, _GENERAL_DISPATCH)
    if "rubyConvexBakeTrace(" in general:
        raise ValueError("General entry must not repeat the speculative bake")
    # The existing attribute before the original function applies to panel.
    # The second function needs its own attribute and the identical dispatch.
    return _once(source, compute, panel) + "\n@compute @workgroup_size(8,8,1)\n" + general + "\n"


source = transform
