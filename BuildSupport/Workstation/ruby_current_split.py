"""Append a queue-free primary shader to the current ruby-response export.

The original workstationShade and all bake/resolve entries stay unchanged.
The host must run workstationFastShade first, then run workstationShade in a
separate pass only where the first result has negative alpha. If any sample
needs fallback, the whole pixel is recomputed by that unchanged shader.
Accepted alpha remains the export's rubyUsedSamples count.
"""

import argparse
from pathlib import Path
import re


def _function(source: str, name: str) -> str:
    marker = "fn " + name + "("
    if source.count(marker) != 1:
        raise ValueError("Current split function ABI changed: " + name)
    start = source.index(marker)
    opening = source.index("{", start)
    depth, end = 1, opening + 1
    while depth and end < len(source):
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    if depth:
        raise ValueError("Unclosed current split function: " + name)
    return source[start:end]


def _once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError("Current split shader marker changed: " + old)
    return source.replace(old, new, 1)


def transform(original: str) -> str:
    """Preserve the exported camera/sample math and isolate its fast calls."""
    if len(original.encode()) > 8 * 1024 * 1024:
        raise ValueError("Current split shader exceeds diagnostic size bound")
    if "fn workstationFastShade(" in original:
        raise ValueError("Current split shader already applied")
    for marker in (
        "var<private> rubyUsedSamples:u32=0u;",
        "@group(0) @binding(2) var<storage,read_write> rubyResponse:RubyResponseData;",
        "@group(0) @binding(3) var<storage,read> rubyPlan:RubyResponseData;",
    ):
        if original.count(marker) != 1:
            raise ValueError("Current split export ABI changed: " + marker)
    wrapper = _function(original, "gemTrace")
    if ("let fastSample=rubyFastTrace(origin,direction,random);" not in wrapper
            or wrapper.count("rubyUsedSamples+=1u;") != 1
            or "return rubyTraceFallback(origin,direction,random);" not in wrapper):
        raise ValueError("Expected the exported current primary/fallback wrapper")
    shade = _function(original, "workstationShade")
    if (shade.count("rubyUsedSamples=0u;") != 1 or
            shade.count("return vec4<f32>(gemDisplay(result/samples),f32(rubyUsedSamples));") != 1):
        raise ValueError("Expected the exported cached-sample alpha diagnostic")
    fast = _once(shade, "fn workstationShade(", "fn workstationFastShade(")
    fast = _once(fast, "        result+=gemTrace(origin,direction,random);",
        "        let fastSample=rubyFastTrace(origin,direction,random);\n"
        "        if(fastSample.w<0.0){rubyUsedSamples=0u;return vec4<f32>(0.0,0.0,0.0,-1.0);}\n"
        "        if(fastSample.w>0.5){rubyUsedSamples+=1u;}\n"
        "        result+=fastSample.xyz;")
    output = original + "\n// Separate primary pass; negative alpha requests full-pixel fallback.\n" + fast + "\n"

    # A separate entry helps only if its entire call graph excludes the general
    # ray queue. Reject future helper changes that accidentally reconnect it.
    names = set(re.findall(r"\bfn\s+(\w+)\s*\(", output))
    pending, seen = ["workstationFastShade"], set()
    while pending:
        name = pending.pop()
        if name in seen:
            continue
        seen.add(name)
        if name in {"gemTrace", "rubyTraceFallback", "rubyBakeTrace", "workstationShade"}:
            raise ValueError("Fast primary reaches the general integrator: " + name)
        body = _function(output, name)
        if "array<GemRay" in body or "pending[" in body:
            raise ValueError("Fast primary reaches a ray queue: " + name)
        pending.extend(set(re.findall(r"\b(\w+)\s*\(", body)) & names - seen)
    return output


source = transform


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.input.resolve() == args.output.resolve():
        parser.error("The diagnostic must not overwrite its source shader")
    generated = transform(args.input.read_text())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(generated)


if __name__ == "__main__":
    main()
