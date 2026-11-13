"""Diagnostic queue-free fallback under the approved studio terminal policy.

The pending queue is empty by induction, independent of geometry. At an
outside dielectric hit, terminal.w == 1 clears reflection; at an inside hit,
transmittedMedium == -1 clears transmission. A membrane clears both. Thus at
most one branch continues, and no queued ray can change global event ordering.

Only rubyTraceFallback changes. Preserve all optical equations, terminal calls
and their accumulation order, engraving, thresholds, depth/event seeds, cache
instrumentation and budget checks. The intrinsic rubyBakeTrace queue remains
untouched: its self-intersection-aware terminal has a different contract.
Fail closed if the concrete terminal, assignment or continuation contract
changes. No general fallback call remains reachable from this specialized
function; retaining one would reintroduce the queue's register allocation.
"""

import re

from ruby_convex_bake import _function, _once


TERMINAL = """fn gemTerminal(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32> {
    // Exterior policy: resolve the studio directly without scene occlusion.
    let environment=gemEnvironment(origin,ray);
    var radiance=environment.xyz;
    if(radiance.x<0.0){radiance=gemFloor(origin+ray*environment.w,-ray);}
    return vec4<f32>(radiance,1.0);
}"""

PENDING = "    var pending:array<GemRay,16>;var count:i32=0;\n"
SCHEDULER = """        // Spend the finite ray budget on the strongest remaining throughput.
        // Depth-first transmission could exhaust the budget in a dark branch
        // while the primary reflected card was still waiting on the stack.
        var strongest:i32=-1;var strongestWeight:f32=current.priority;
        for(var candidate:i32=0;candidate<16;candidate++){
            if(candidate>=count){break;}
            var weight:f32=pending[candidate].priority;
            if(weight>strongestWeight){strongest=candidate;strongestWeight=weight;}
        }
        if(strongest>=0){var swapRay:GemRay=current;current=pending[strongest];pending[strongest]=swapRay;}
"""
MEMBRANE_CONTINUATION = """            if(retainReflection&&retainTransmission){
                if(reflectPriority>transmitPriority){current=reflection;if(count<16){pending[count]=transmission;count++;}}
                else{current=transmission;if(count<16){pending[count]=reflection;count++;}}
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"""
DIELECTRIC_CONTINUATION = """            if(retainReflection&&retainTransmission){
                if(reflectPriority>transmitPriority){
                    current=reflection;if(count<16){pending[count]=transmission;count++;}
                }else{
                    current=transmission;if(count<16){pending[count]=reflection;count++;}
                }
            }else if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"""
LINEAR_CONTINUATION = """            if(retainReflection){current=reflection;}
            else if(retainTransmission){current=transmission;}
            else{finished=true;}
"""
TAIL = """        if(finished){
            if(count==0){break;}count--;current=pending[count];
        }
"""


def _count(source: str, marker: str, count: int) -> None:
    if source.count(marker) != count:
        raise ValueError("Studio linear shader contract changed: " + marker)


def _writes(source: str, name: str, count: int) -> None:
    # Known declaration/assignment markers account for all writes to the
    # control variables. Reject an added mutation that could revive a branch.
    pattern = (r"\b" + re.escape(name) +
               r"\s*(?::\s*[A-Za-z_]\w*)?\s*(?:[+*/%&|^\-]=|=(?!=)|\+\+|--)")
    if len(re.findall(pattern, source)) != count:
        raise ValueError("Studio linear control writes changed: " + name)


def transform(source: str) -> str:
    """Remove the provably empty pending queue only from rubyTraceFallback."""
    if _function(source, "gemTerminal") != TERMINAL:
        raise ValueError("Studio linear requires the exact unconditional studio terminal")
    trace = _function(source, "rubyTraceFallback")
    _count(trace, "var current:GemRay=GemRay(origin,-1,direction,0,vec3<f32>(1.0),1.0);", 1)
    _count(trace, "for(var event:i32=0;event<64;event++)", 1)
    _count(trace, "if(f32(event)>=gemRow(8).w){break;}", 1)
    _count(trace, "var inside:bool=current.medium>=0;", 1)
    _count(trace, "var transmittedMedium:i32=hit.panel;if(inside){transmittedMedium=-1;}", 1)
    _count(trace, "if(retainReflection&&retainTransmission){", 2)
    _count(trace, "gemTerminal(", 4)
    for branch, direction, weight, medium in (
            ("Reflection", "reflected", "reflectWeight", "current.medium"),
            ("Transmission", "transmitted", "transmitWeight", "transmittedMedium")):
        call = "gemTerminal(point+" + direction + "*epsilon," + direction + ")"
        _count(trace, call, 2)
        for guard in ("retain" + branch, "retain" + branch + "&&" + medium + "<0"):
            terminal = ("            if(" + guard + "){\n"
                        "                var terminal:vec4<f32>=" + call + ";\n"
                        "                if(terminal.w>0.5){result+=" + weight +
                        "*terminal.xyz;retain" + branch + "=false;}\n            }\n")
            _count(trace, terminal, 1)
        _count(trace, "var retain" + branch + ":bool=", 2)
        _writes(trace, "retain" + branch, 4)
    _writes(trace, "inside", 1)
    _writes(trace, "transmittedMedium", 2)
    _writes(trace, "current.medium", 0)
    changed = _once(trace, PENDING, "")
    changed = _once(changed, SCHEDULER, "")
    changed = _once(changed, MEMBRANE_CONTINUATION, LINEAR_CONTINUATION)
    changed = _once(changed, DIELECTRIC_CONTINUATION, LINEAR_CONTINUATION)
    changed = _once(changed, TAIL, "        if(finished){break;}\n")
    # One initial ray and two reflection/transmission continuation pairs.
    _writes(changed, "current", 5)
    for forbidden in ("pending", "strongest", "swapRay", "count", "retainReflection&&retainTransmission"):
        if forbidden in changed:
            raise ValueError("Studio linear still depends on queue: " + forbidden)
    return _once(source, trace, changed)


source = transform
