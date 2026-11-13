#!/usr/bin/env python3
"""Default-off, source-only engraving factor proposal. No production edits.

transform(source, variant="off") returns the unchanged source by default.
Variants: metadata, cosine, navigation-zero, all.
Metadata/cosine retain canonical arithmetic and positive-light contribution
order. Navigation-zero additionally relies on immutable finite authored normals;
a conservative runtime bound proves the remaining light arithmetic finite.
GPU equivalence/performance are unverified. The parent must validate all variants.
"""
from pathlib import Path
import argparse

MAX_SOURCE_BYTES = 2 * 1024 * 1024
MARKER = "// GEM_ENGRAVING_FACTOR_PROPOSAL:"

def once(source, old, new):
    if source.count(old) != 1:
        raise ValueError(f"engraving factor marker changed ({source.count(old)}): {old[:120]}")
    return source.replace(old, new, 1)

def span(source, name):
    marker = f"fn {name}("
    if source.count(marker) != 1:
        raise ValueError(f"engraving function changed: {name}")
    start = source.index(marker)
    at = source.index("{", start)
    end, depth = at + 1, 1
    while depth and end < len(source):
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    if depth:
        raise ValueError(f"unclosed engraving function: {name}")
    return start, end

def function(source, name):
    start, end = span(source, name)
    return source[start:end]

def edit(source, name, old, new):
    start, end = span(source, name)
    return source[:start] + once(source[start:end], old, new) + source[end:]

GLYPH_STRUCT = """
struct GemFactorGlyphFilter {
    atlas:vec4<f32>,
    grid:vec4<f32>,
    lo:vec2<i32>,
    hi:vec2<i32>,
    blend:f32,
}
"""

GLYPH_PREPARE = """
fn gemFactorMakeGlyphFilter(footprint:f32)->GemFactorGlyphFilter {
    let atlas=gemRow(224);
    let grid=gemRow(213);
    // Same left-associated float32 expression as gemEngravedDensity.
    var lod:f32=clamp(log2(max(footprint*atlas.y/atlas.z,1.0)),0.0,7.0);
    var lo:i32=i32(floor(lod));var hi:i32=min(lo+1,7);
    return GemFactorGlyphFilter(atlas,grid,
        gemFactorGlyphMip(atlas,lo),gemFactorGlyphMip(atlas,hi),fract(lod));
}
"""

CONTROL_STRUCT = """
struct GemFactorControlFilter {
    region:vec4<f32>,
    fractureScale:f32,
    raster:i32,
    loMip:vec4<f32>,
    hiMip:vec4<f32>,
    blend:f32,
}
"""

CONTROL_PREPARE = """
fn gemFactorMakeControlFilter(descriptor:i32,footprint:f32)->GemFactorControlFilter {
    var region:vec4<f32>=gemRow(descriptor);var depth:vec4<f32>=gemRow(descriptor+1);
    var packet:vec4<f32>=gemRow(descriptor+2);var size:vec2<f32>=gemRow(i32(packet.z)).xy;
    var resolution:f32=max(size.x/region.z,size.y/region.w);
    var lod:f32=clamp(log2(max(footprint*resolution,1.0)),0.0,packet.y-1.0);
    var lo:i32=i32(floor(lod));var hi:i32=min(lo+1,i32(packet.y)-1);
    return GemFactorControlFilter(region,depth.w*gemRow(213).x,
        i32(gemRow(244).x)+i32(packet.x),
        gemRow(i32(packet.z)+lo),gemRow(i32(packet.z)+hi),fract(lod));
}
"""

CONTROL_DENSITY = """
fn gemFactorControlDensity(point:vec2<f32>,shift:vec2<f32>,layer:i32,
                           prepared:GemFactorControlFilter)->f32 {
    var uv:vec2<f32>=(point-prepared.region.xy-shift*prepared.fractureScale)/prepared.region.zw;
    return mix(gemFactorControlLevel(uv,prepared.loMip,prepared.raster,layer),
               gemFactorControlLevel(uv,prepared.hiMip,prepared.raster,layer),prepared.blend);
}
"""

def metadata(source):
    # Derive samplers from their canonical function bodies. Only immutable
    # metadata/LOD are passed explicitly; texel unpacking and nested mix order
    # remain literally unchanged.
    texel = function(source, "gemDensityTexel")
    texel = once(texel, "fn gemDensityTexel(", "fn gemFactorGlyphTexel(")
    texel = once(texel, "layer:i32)->f32 {", "layer:i32,atlas:vec4<f32>)->f32 {")
    if texel.count("gemRow(224)") != 2:
        raise ValueError("glyph atlas descriptor reads changed")
    texel = texel.replace("gemRow(224)", "atlas")
    level = function(source, "gemDensityLevel")
    split = level.index("    var p:vec2<f32>")
    body_start = level.index("{") + 1
    mip_body = level[body_start:split]
    if mip_body.count("gemRow(224)") != 1:
        raise ValueError("glyph mip descriptor changed")
    mip_body = mip_body.replace("gemRow(224)", "atlas")
    mip = ("fn gemFactorGlyphMip(atlas:vec4<f32>,level:i32)->vec2<i32> {"
           + mip_body + "    return vec2<i32>(edge,offset);\n}")
    level = level[:body_start] + "\n    var edge:i32=info.x;var offset:i32=info.y;\n" + level[split:]
    level = once(level, "fn gemDensityLevel(", "fn gemFactorGlyphLevel(")
    level = once(level, "level:i32,layer:i32)->f32 {", "info:vec2<i32>,layer:i32,atlas:vec4<f32>)->f32 {")
    for coord in ["x,y", "x+1,y", "x,y+1", "x+1,y+1"]:
        level = once(level, f"gemDensityTexel(glyph,{coord},edge,offset,layer)",
                     f"gemFactorGlyphTexel(glyph,{coord},edge,offset,layer,atlas)")
    density = function(source, "gemEngravedDensity")
    density = once(density, "fn gemEngravedDensity(", "fn gemFactorGlyphDensity(")
    density = once(density, "footprint:f32,layer:i32)->f32 {",
                   "layer:i32,prepared:GemFactorGlyphFilter)->f32 {")
    density = once(density, "var grid:vec4<f32>=gemRow(213);", "var grid:vec4<f32>=prepared.grid;")
    density = once(density, "/gemRow(224).z;", "/prepared.atlas.z;")
    lod = """    var lod:f32=clamp(log2(max(footprint*gemRow(224).y/gemRow(224).z,1.0)),0.0,7.0);
    var lo:i32=i32(floor(lod));var hi:i32=min(lo+1,7);
"""
    density = once(density, lod, "")
    density = once(density,
        "mix(gemDensityLevel(uv,glyph,lo,layer),gemDensityLevel(uv,glyph,hi,layer),fract(lod))",
        "mix(gemFactorGlyphLevel(uv,glyph,prepared.lo,layer,prepared.atlas),gemFactorGlyphLevel(uv,glyph,prepared.hi,layer,prepared.atlas),prepared.blend)")
    source += "\n" + GLYPH_STRUCT + mip + "\n" + GLYPH_PREPARE + texel + "\n" + level + "\n" + density + "\n"

    control = function(source, "gemControlDensity")
    # Fail closed if any expression mirrored by the prepared struct changes.
    for marker in [
        "var region:vec4<f32>=gemRow(descriptor);var depth:vec4<f32>=gemRow(descriptor+1);",
        "var packet:vec4<f32>=gemRow(descriptor+2);var size:vec2<f32>=gemRow(i32(packet.z)).xy;",
        "(point-region.xy-shift*(depth.w*gemRow(213).x))/region.zw",
        "var resolution:f32=max(size.x/region.z,size.y/region.w);",
        "var lod:f32=clamp(log2(max(footprint*resolution,1.0)),0.0,packet.y-1.0);",
        "var lo:i32=i32(floor(lod));var hi:i32=min(lo+1,i32(packet.y)-1);",
        "mix(gemControlDensityLevel(uv,descriptor,lo,layer),\n               gemControlDensityLevel(uv,descriptor,hi,layer),fract(lod))",
    ]:
        if control.count(marker) != 1:
            raise ValueError("control density equation changed: " + marker)
    ctexel = function(source, "gemControlDensityTexel")
    ctexel = once(ctexel, "fn gemControlDensityTexel(descriptor:i32,level:i32,",
                  "fn gemFactorControlTexel(mip:vec4<f32>,raster:i32,")
    ctexel = once(ctexel, "    var packet:vec4<f32>=gemRow(descriptor+2);\n    var mip:vec4<f32>=gemRow(i32(packet.z)+level);\n", "")
    ctexel = once(ctexel, "i32(gemRow(244).x)+i32(packet.x)+texel/2", "raster+texel/2")
    clevel = function(source, "gemControlDensityLevel")
    clevel = once(clevel, "fn gemControlDensityLevel(uv:vec2<f32>,descriptor:i32,level:i32,",
                  "fn gemFactorControlLevel(uv:vec2<f32>,mip:vec4<f32>,raster:i32,")
    clevel = once(clevel,
        "var packet:vec4<f32>=gemRow(descriptor+2);var size:vec2<f32>=gemRow(i32(packet.z)+level).xy;",
        "var size:vec2<f32>=mip.xy;")
    for coord in ["x,y", "x+1,y", "x,y+1", "x+1,y+1"]:
        clevel = once(clevel, f"gemControlDensityTexel(descriptor,level,{coord},layer)",
                      f"gemFactorControlTexel(mip,raster,{coord},layer)")
    source += "\n" + CONTROL_STRUCT + CONTROL_PREPARE + ctexel + "\n" + clevel + "\n" + CONTROL_DENSITY

    pixel = "    var pixel:f32=2.0*gemRow(3).w/(gemRow(8).y*pose.w);\n"
    for name in ["gemTimelineEngraving", "gemPluginNavigationEngraving"]:
        source = edit(source, name, pixel, pixel
            + "    let glyphFilter=gemFactorMakeGlyphFilter(pixel*0.75/font);\n")
        source = edit(source, name,
            "gemEngravedDensity(em,glyph,vec2<f32>(0.0),pixel*0.75/font,layer)",
            "gemFactorGlyphDensity(em,glyph,vec2<f32>(0.0),layer,glyphFilter)")
    # Control preparation follows the existing descriptor bounding-box miss
    # check, so completely rejected text regions pay no preparation cost.
    marker = "    var sigma:vec3<f32>=gemRow(base+1).xyz;var result:vec3<f32>=vec3<f32>(0.0);var transmission:f32=1.0;\n"
    source = edit(source, "gemControlEngraving", marker,
        "    let controlFilter=gemFactorMakeControlFilter(descriptor,pixel*0.75);\n" + marker)
    source = edit(source, "gemControlEngraving",
        "gemControlDensity(point.xy,descriptor,shift,pixel*0.75,layer)",
        "gemFactorControlDensity(point.xy,shift,layer,controlFilter)")
    return source

def cosine(source):
    # All positive-light floating point operations retain the original order.
    # A nonpositive/NaN cosine made the old contribution condition false anyway.
    for name, query in [
        ("gemControlEngravingLight", "gemControlExit(point,l,medium,distance)"),
        ("gemTimelineEngravingLight", "gemTimelineHit(point,l,medium,distance)"),
    ]:
        line = f"        var exitHit:GemHit={query};"
        # Replace the balanced function atomically: reparsing after inserting
        # just the opening brace would consume following function bodies.
        start, end = span(source, name)
        body = once(source[start:end], line, "        if(cosine>0.0){\n" + line)
        body = once(body, "        }\n    }\n    return illumination",
                    "        }\n        }\n    }\n    return illumination")
        source = source[:start] + body + source[end:]
    return source

ZERO_SAFE = """
// This bound proves the original light arithmetic finite for current authored
// finite normals. Successful gemControlExit distances exceed 1e-7. With these
// bounds, even its inverse-square worst case stays below 3e30 after both SSLE
// multipliers. Otherwise the original light function is called.
fn gemFactorZeroLightSafe(point:vec3<f32>,medium:i32,scale:f32)->bool {
    if(!all(abs(point)<=vec3<f32>(10000.0))||!(scale>=0.000000001&&scale<=10000.0)){return false;}
    let base=gemPanelRow(medium);let material=gemRow(base);let sigma=gemRow(base+1).xyz;
    if(!(material.y>=1.0&&material.y<=4.0)||!all(sigma>=vec3<f32>(0.0))||!all(sigma<=vec3<f32>(1000.0))){return false;}
    let ambient=gemRow(5).xyz;let tint=gemRow(214).xyz;let gain=gemRow(223).w;
    if(!all(ambient>=vec3<f32>(0.0))||!all(ambient<=vec3<f32>(1000.0))){return false;}
    if(!all(tint>=vec3<f32>(0.0))||!all(tint<=vec3<f32>(1000.0))||!(gain>=0.0&&gain<=1000.0)){return false;}
    for(var light:i32=0;light<3;light++){
        let at=16+5*light;
        let position=gemRow(at).xyz;let normal=gemRow(at+3).xyz;let emission=gemRow(at+4).xyz;
        let width=gemRow(at+1).w;let height=gemRow(at+2).w;
        if(!all(abs(position)<=vec3<f32>(10000.0))||!all(abs(normal)<=vec3<f32>(2.0))){return false;}
        if(!all(emission>=vec3<f32>(0.0))||!all(emission<=vec3<f32>(1000.0))){return false;}
        if(!(width>=0.0&&width<=1000.0&&height>=0.0&&height<=1000.0)){return false;}
    }
    return true;
}
"""

def navigation_zero(source):
    # Do not extend this to a nonzero density threshold. Original alpha and the
    # original accumulation/transmission operations are retained, including the
    # zero addition. This is conditional exactness, not a geometry substitute.
    original = function(source, "gemControlExit")
    for marker in [
        "if(far*pose.w>0.0000001&&far*pose.w<farLimit){return GemHit(far*pose.w,normal,medium);}",
        "if(t>0.0000001&&t<hit.distance){",
    ]:
        if original.count(marker) != 1:
            raise ValueError("control exit distance proof changed")
    normal = function(source, "gemControlNormal")
    if normal.count("gemSafeUnit(") != 2:
        raise ValueError("authored control normal policy changed")
    old = "        result+=transmission*alpha*gemControlEngravingLight(pose.xyz+point*pose.w,medium,pose.w);"
    new = """        let world=pose.xyz+point*pose.w;
        if(alpha==0.0&&gemFactorZeroLightSafe(world,medium,pose.w)){
            result+=transmission*alpha*vec3<f32>(0.0);
        }else{
            result+=transmission*alpha*gemControlEngravingLight(world,medium,pose.w);
        }"""
    source = edit(source, "gemPluginNavigationEngraving", old, new)
    return source + "\n" + ZERO_SAFE

def transform(source, variant="off"):
    if variant == "off":
        return source
    if variant not in {"metadata", "cosine", "navigation-zero", "all"}:
        raise ValueError("unknown engraving variant")
    if len(source.encode()) > MAX_SOURCE_BYTES or MARKER in source or "fn gemFactor" in source:
        raise ValueError("unexpected/repeated engraving factor input")
    if variant in {"metadata", "all"}:
        source = metadata(source)
    if variant in {"cosine", "all"}:
        source = cosine(source)
    if variant in {"navigation-zero", "all"}:
        source = navigation_zero(source)
    return f"{MARKER} {variant}\n" + source

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--variant", choices=["off","metadata","cosine","navigation-zero","all"], default="off")
    args = parser.parse_args()
    if args.source.stat().st_size > MAX_SOURCE_BYTES:
        raise SystemExit("source exceeds 2MiB bound")
    source = args.source.read_text()
    result = transform(source, args.variant)
    # No mutation of source, production files, or existing diagnostic outputs.
    with args.output.open("x") as out:
        out.write(result)
    print(f"Wrote {args.variant} proposal to {args.output}; GPU validation remains required")

if __name__ == "__main__":
    main()
