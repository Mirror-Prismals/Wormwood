"""Diagnostic single-continuation bake for convex ruby panels only.

The dielectric arithmetic is copied from the exported production rubyBakeTrace,
not independently simplified. Panel entry, exit, terminal self-hit checks,
sample sequence, absorption, thresholds, budgets and rubyRecord remain intact.
The kernel has no generic ray queue, bubble, opaque or control-material branch.
Any terminal reentry or need for two continuing branches rejects the attempt;
all bake accumulators are reset before the original kernel reruns that sample.

Apply transform(source) to the current exported production WGSL. No application
loads this module. It adds no bindings and uses the already-unused cache row29
as [attempted, accepted, fallback, attempted_event_count] per sample. Event count
excludes any subsequent canonical rerun. Controls report four zeros.
"""


def _once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError("Convex ruby marker changed: " + old.splitlines()[0])
    return source.replace(old, new, 1)


def _closing(source: str, opening: int) -> int:
    end = opening + 1
    depth = 1
    while depth and end < len(source):
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    if depth:
        raise ValueError("Unclosed convex ruby block")
    return end


def _block(source: str, marker: str) -> tuple[int, int, int, int]:
    """Return marker start, body start, body end and block end."""
    if source.count(marker) != 1:
        raise ValueError("Expected exactly one convex ruby block: " + marker)
    start = source.index(marker)
    opening = source.index("{", start)
    end = _closing(source, opening)
    return start, opening + 1, end - 1, end


def _function(source: str, name: str) -> str:
    start, _, _, end = _block(source, "fn " + name + "(")
    return source[start:end]


_OLD_CONTINUATION = """            if(retainReflection&&retainTransmission){
                if(reflectPriority>transmitPriority){
                    current=reflection;if(count<16){pending[count]=transmission;count++;}
                }else{
                    current=transmission;if(count<16){pending[count]=reflection;count++;}
                }
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"""

_NEW_CONTINUATION = """            if(retainReflection&&retainTransmission){
                rubyConvexUnsupported=true;return vec3<f32>(0.0);
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"""

_SUPPORT = r"""
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
// Keep the full authored entry test until a separate escape certificate is
// proven. No control BVH is reachable from this diagnostic panel kernel.
fn rubyConvexTerminal(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>,depth:i32)->vec4<f32> {
    let hit=gemEnterPanel(origin,direction,gemInverseRay(direction),rubyBakeTarget,1e20);
    if(hit.panel>=0&&hit.distance>0.0){return vec4<f32>(0.0);}
    rubyRecord(origin,direction,weight,depth);
    return vec4<f32>(0.0,0.0,0.0,1.0);
}
"""

_HEAD = r"""
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
"""

_TAIL = r"""
        if(finished){break;}
    }
    return result;
}
"""

_DISPATCH = """        rubyConvexStats=vec4<f32>(0.0);rubyConvexUnsupported=false;
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
        }"""


def transform(source: str) -> str:
    """Return current production shader with guarded convex-panel baking."""
    if "fn rubyConvexBakeTrace(" in source:
        raise ValueError("Convex ruby bake has already been applied")
    original = _function(source, "rubyBakeTrace")
    for name in ("gemEngraving(", "gemControlEngraving(",
                 "gemPluginNavigationEngraving(", "gemTimelineEngraving("):
        if name in original:
            raise ValueError("Requires unengraved production intrinsic bake: " + name)
    if "for(var event:i32=0;event<64;event++)" not in original:
        raise ValueError("Convex ruby event bound changed")
    if "if(f32(event)>=gemRow(8).w){break;}" not in original:
        raise ValueError("Convex ruby event budget changed")
    if "var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);" not in original:
        raise ValueError("Convex ruby initial ray changed")

    _, begin, end, _ = _block(original, "if(!finished&&!membrane){")
    material = original[begin:end]
    opaque_start, _, _, opaque_end = _block(material, "if(material.w<0.0001){")
    prefix = material[:opaque_start]
    remaining = material[opaque_end:]
    if not remaining.lstrip().startswith("else{"):
        raise ValueError("Convex ruby dielectric branch changed")
    # else{} occurs inside the dielectric branch too. Parse the leading branch
    # directly rather than accepting an ambiguous later material operation.
    opening = remaining.index("else{") + len("else")
    block_end = _closing(remaining, opening)
    begin, end = opening + 1, block_end - 1
    dielectric = remaining[begin:end]
    if remaining[block_end:].strip():
        raise ValueError("Unexpected work after convex ruby dielectric branch")
    for direction, weight in (("reflected", "reflectWeight"), ("transmitted", "transmitWeight")):
        terminal = ("if(terminal.w>0.5){result+=" + weight + "*terminal.xyz;retain" +
                    ("Reflection" if direction == "reflected" else "Transmission") + "=false;}")
        dielectric = _once(dielectric, terminal, terminal +
            "\n                else{rubyConvexUnsupported=true;return vec3<f32>(0.0);}")
    dielectric = _once(dielectric, _OLD_CONTINUATION, _NEW_CONTINUATION)
    if dielectric.count("rubyBakeTerminal(") != 2:
        raise ValueError("Convex ruby terminal call count changed")
    dielectric = dielectric.replace("rubyBakeTerminal(", "rubyConvexTerminal(")
    if "pending[" in dielectric or "count" in dielectric:
        raise ValueError("Convex ruby still depends on the generic ray queue")
    kernel = (_HEAD + prefix +
        "if(material.w<0.0001){rubyConvexUnsupported=true;return vec3<f32>(0.0);}\n" +
        dielectric + _TAIL)

    compute = _function(source, "cs_ruby_response")
    changed = _once(compute,
        "        let radiance=rubyBakeTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);", _DISPATCH)
    changed = _once(changed,
        "        rubyResponse.rows[address+29u]=vec4<f32>(0.0);",
        "        rubyResponse.rows[address+29u]=rubyConvexStats;")
    return _once(source, compute, changed) + _SUPPORT + kernel


source = transform
