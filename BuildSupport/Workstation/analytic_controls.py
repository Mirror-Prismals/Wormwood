"""Private control-interior experiments; never edits production assets/shaders.

python3 BuildSupport/Workstation/analytic_controls.py --shader canonical.wgsl
  --frame complete-frame.bin --output /tmp/probe --mode exact-facets
Modes: exact-facets, analytic-construction. Produces .wgsl, .bin and .json.
The caller runs compute_probe separately; this script performs source math only.
"""
import argparse
import array
from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path
import random
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
MATERIALS = [0, 2, 4, 6, 8, 10, 11]
MAX_RESIDUAL_M = 2e-5


def dot(a, b): return sum(x * y for x, y in zip(a, b))
def sub(a, b): return [x - y for x, y in zip(a, b)]
def add(a, b): return [x + y for x, y in zip(a, b)]
def mul(a, b): return [x * b for x in a]
def cross(a, b): return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
def unit(a): return mul(a, 1 / math.sqrt(dot(a, a)))
def f32(v): return struct.unpack("<f", struct.pack("<f", v))[0]


def read_rows(path):
    values = array.array("f")
    values.frombytes(path.read_bytes())
    if sys.byteorder != "little": values.byteswap()
    assert len(values) % 4 == 0
    return [list(values[i:i+4]) for i in range(0, len(values), 4)]


def original_triangles(rows, base, material):
    interior = rows[base + 40 * material + 2]
    if interior[2] > 0: return []  # Already has an analytic implementation.
    start, end = map(int, interior[:2])
    assert 0 < start < end < len(rows), (material, interior)
    result = []
    for node in range(start, end, 3):
        first, count = int(rows[node+1][3]), int(rows[node+2][0])
        for i in range(count):
            at = first + i * 6
            a, b, c = rows[at:at+3]
            e1, e2 = b[:3], c[:3]
            if b[3] < .5: e1, e2 = sub(e1, a[:3]), sub(e2, a[:3])
            normals = [v[:3] for v in rows[at+3:at+6]]
            assert normals[0] == normals[1] == normals[2], "Candidate only supports authored flat normals"
            result.append({"address": at, "rank": len(result), "a": a[:3], "e1": e1, "e2": e2,
                           "vertices": [a[:3], add(a[:3], e1), add(a[:3], e2)], "normal": normals[0]})
    assert result
    return result


def triangle_hit(t, origin, ray):
    h = cross(ray, t["e2"])
    determinant = dot(t["e1"], h)
    if abs(determinant) < 1e-12: return None
    offset = sub(origin, t["a"])
    u = dot(offset, h) / determinant
    if u < 0 or u > 1: return None
    q = cross(offset, t["e1"])
    v = dot(ray, q) / determinant
    if v < 0 or u + v > 1: return None
    distance = dot(t["e2"], q) / determinant
    normal = unit(t["normal"])
    if distance <= 1e-7 or dot(ray, normal) <= 0: return None
    return distance, normal, t["rank"]


def nearest(triangles, origin, ray):
    hits = [hit for t in triangles if (hit := triangle_hit(t, origin, ray)) is not None]
    return min(hits, key=lambda h: (h[0], h[2])) if hits else None


def facet_groups(triangles):
    groups = defaultdict(list)
    for triangle in triangles: groups[tuple(triangle["normal"])].append(triangle)
    result = []
    for normal, ts in groups.items():
        vertices = [v for t in ts for v in t["vertices"]]
        distances = [dot(normal, v) for v in vertices]
        result.append({"normal": list(normal), "lo": min(distances)-2e-7, "hi": max(distances)+2e-7,
                       "minimum": [min(v[i] for v in vertices)-2e-7 for i in range(3)],
                       "maximum": [max(v[i] for v in vertices)+2e-7 for i in range(3)], "triangles": ts})
    return result


def facet_hit(groups, origin, ray):
    hits = []
    for group in groups:
        box_near, box_far = 0., 1e20
        for axis in range(3):
            inverse = (-1. if ray[axis] < 0 else 1.) / max(abs(ray[axis]), 1e-8)
            a = (group["minimum"][axis]-origin[axis])*inverse
            b = (group["maximum"][axis]-origin[axis])*inverse
            box_near, box_far = max(box_near, min(a,b)), min(box_far, max(a,b))
        if box_far < box_near: continue
        denom = dot(group["normal"], ray)
        start = dot(group["normal"], origin)
        if abs(denom) > 1e-12:
            lo, hi = sorted(((group["lo"]-start)/denom, (group["hi"]-start)/denom))
            if hi <= 1e-7: continue
        hits.extend(hit for t in group["triangles"] if (hit := triangle_hit(t, origin, ray)) is not None)
    return min(hits, key=lambda h: (h[0], h[2])) if hits else None


def matrix_product(a, b):
    return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def node_matrix(node):
    if "matrix" in node: return [[node["matrix"][j*4+i] for j in range(4)] for i in range(4)]
    x, y, z, w = node.get("rotation", [0, 0, 0, 1])
    s, t = node.get("scale", [1, 1, 1]), node.get("translation", [0, 0, 0])
    m = [[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w), t[0]],
         [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w), t[1]],
         [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y), t[2]], [0,0,0,1]]
    for i in range(3):
        for j in range(3): m[i][j] *= s[j]
    return m


def source_nodes(asset_root):
    art = asset_root / "Art/AudioWorkstation"
    files = ["GemButtons/GemButton_" + n + ".glb" for n in ["Rewind", "Stop", "Record", "Play"]]
    files += ["GemFadersMeters/GemFader_Ivory.glb", "GemFadersMeters/GemMeter_Ivory.glb"]
    result, offset = {}, 0
    identity = [[float(i == j) for j in range(4)] for i in range(4)]
    for name in files:
        raw = (art / name).read_bytes()
        size = struct.unpack_from("<I", raw, 12)[0]
        gltf = json.loads(raw[20:20+size])
        def visit(index, parent):
            node = gltf["nodes"][index]
            m = matrix_product(parent, node_matrix(node))
            extras = node.get("extras", {})
            if "nominal_width_m" in extras and "mesh" in node:
                materials = {p["material"] + offset for p in gltf["meshes"][node["mesh"]]["primitives"]}
                assert len(materials) == 1
                result[next(iter(materials))] = {"name": node["name"], "extras": extras, "matrix": m, "source": name}
            for child in node.get("children", []): visit(child, m)
        for node in gltf["scenes"][gltf.get("scene", 0)]["nodes"]: visit(node, identity)
        offset += len(gltf["materials"])
    return result


def source_gem_planes(width, height):
    """The authoring script's exact26-plane cut, in metre/table coordinates."""
    minor = min(width, height)
    a, b, scale = width/minor, height/minor, minor*.5
    planes = [([0., 0., 1.], .30), ([0., 0., -1.], .85)]
    for ring in range(3):
        for side in range(8):
            angle = side*math.pi/4 + (math.pi/8 if ring == 2 else 0)
            x, y = math.cos(angle), math.sin(angle)
            support = abs(x)*a + abs(y)*b - .18*min(abs(x), abs(y))
            slope = support*.86/(.85-.07)
            normal = [x, y, 0. if ring == 0 else .31/.30 if ring == 1 else -slope]
            distance = support+slope*.07 if ring == 2 else support
            planes.append((normal, distance))
    return [(n, scale*(d-.30*n[2])) for n, d in planes]


def source_cutter(name):
    if "Fader" in name and "Housing" in name:
        width, height, cy = .020, .134, .011
    elif "Meter" in name and "Housing" in name:
        width, height, cy = .013, .158, 0.
    else: return []
    x, y, c = width*.5, height*.5, .0012
    outline = [(-x+c,-y),(x-c,-y),(x,-y+c),(x,y-c),(x-c,y),(-x+c,y),(-x,y-c),(-x,-y+c)]
    planes = [([0.,0.,-1.], .0018), ([0.,0.,1.], .003)]
    for a, b in zip(outline, outline[1:]+outline[:1]):
        n = unit([b[1]-a[1], -(b[0]-a[0]), 0.])
        planes.append((n, n[0]*a[0]+n[1]*(a[1]+cy)))
    return planes


def transformed_planes(planes, matrix, triangles, cutter=False):
    # Blender -> glTF -> composed GLB node transform -> Blender.
    def transform(v, translation=False):
        q = [v[0], v[2], -v[1]]
        p = [sum(matrix[i][j]*q[j] for j in range(3)) + (matrix[i][3] if translation else 0) for i in range(3)]
        return [p[0], -p[2], p[1]]
    translation = transform([0,0,0], True)
    result = []
    for n, d in planes:
        world = transform(n)
        length = math.sqrt(dot(world, world))
        normal, distance = mul(world, 1/length), (d+dot(world, translation))/length
        # Preserve the authored optical normal where that face survives the cut.
        target = mul(normal, -1 if cutter else 1)
        candidates = []
        for triangle in triangles:
            deviation = max(abs(dot(normal, v)-distance) for v in triangle["vertices"])
            if deviation <= MAX_RESIDUAL_M and dot(unit(triangle["normal"]), target) > .999:
                candidates.append((deviation, triangle["rank"], triangle["normal"]))
        shading = min(candidates)[2] if candidates else target
        result.append({"normal": normal, "distance": distance, "shading": list(shading)})
    return result


def interval(planes, origin, ray):
    near, far, entry, exit_normal = -1e20, 1e20, None, None
    for plane in planes:
        denom = dot(plane["normal"], ray)
        distance = plane["distance"] - dot(plane["normal"], origin)
        if abs(denom) < 1e-12:
            if distance < 0: return None
        elif denom < 0:
            t = distance/denom
            if t > near: near, entry = t, plane["shading"]
        else:
            t = distance/denom
            if t < far: far, exit_normal = t, plane["shading"]
        if far < near: return None
    return near, far, entry, exit_normal


def construction_hit(shape, origin, ray):
    outer = interval(shape["outer"], origin, ray)
    if outer is None: return None
    near, far, _, normal = outer
    cut = interval(shape["cutter"], origin, ray) if shape["cutter"] else None
    if cut and cut[1] > near and cut[0] < far:
        # First remaining segment ends at entry into the cutter. Its stored
        # shading normal already points outward from the difference solid.
        stop = min(far, cut[0])
        if stop >= near and stop > 1e-7: return stop, unit(cut[2]), -1
        start = max(near, cut[1])
        if far >= start and far > 1e-7: return far, unit(normal), -1
        return None
    return (far, unit(normal), -1) if far > 1e-7 else None


def validate_construction(node, triangles):
    extras = node["extras"]
    assert extras["source_depth"] == .85 and extras["source_crown"] == .31
    assert extras["pavilion_rotation_deg"] == 22.5
    matrix = node["matrix"]
    assert all(abs(sum(matrix[k][i]*matrix[k][j] for k in range(3)) - float(i == j)) < 1e-5
               for i in range(3) for j in range(3)), "Source transform is not rigid"
    outer = transformed_planes(source_gem_planes(extras["nominal_width_m"], extras["nominal_height_m"]), matrix, triangles)
    cutter = transformed_planes(source_cutter(node["name"]), matrix, triangles, cutter=True)
    all_planes = outer + cutter
    # Each triangle must lie close to one construction face. This reports a
    # planar residual, not a certified Hausdorff or radiance bound.
    face_residuals = [min(max(abs(dot(p["normal"], v)-p["distance"]) for v in t["vertices"])
                          for p in all_planes) for t in triangles]
    maximum = max(face_residuals)
    return {"outer": outer, "cutter": cutter, "max_triangle_plane_residual_m": maximum,
            "accepted": maximum <= MAX_RESIDUAL_M,
            "qualification": "Parametric-source candidate; bounded triangle-to-construction-plane residual, not exact GLB geometry or radiance equivalence."}


def comparisons(triangles, groups, shape, count, seed):
    rng = random.Random(seed)
    vertices = [v for t in triangles for v in t["vertices"]]
    low = [min(v[i] for v in vertices) for i in range(3)]
    high = [max(v[i] for v in vertices) for i in range(3)]
    result = {"rays": count, "exact_facet_mismatches": 0, "construction_miss_mismatches": 0,
              "construction_max_distance_delta_m": 0., "construction_max_normal_delta": 0.}
    for _ in range(count):
        origin = [rng.uniform(low[i], high[i]) for i in range(3)]
        ray = unit([rng.uniform(-1,1) for _ in range(3)])
        baseline, guided = nearest(triangles, origin, ray), facet_hit(groups, origin, ray)
        if baseline != guided: result["exact_facet_mismatches"] += 1
        if shape and shape["accepted"]:
            ideal = construction_hit(shape, origin, ray)
            if (baseline is None) != (ideal is None): result["construction_miss_mismatches"] += 1
            elif baseline:
                result["construction_max_distance_delta_m"] = max(result["construction_max_distance_delta_m"], abs(baseline[0]-ideal[0]))
                result["construction_max_normal_delta"] = max(result["construction_max_normal_delta"], max(abs(a-b) for a,b in zip(baseline[1],ideal[1])))
    assert result["exact_facet_mismatches"] == 0, result
    return result


EXACT_WGSL = r'''
fn probeFacetTriangle(at:i32,rank:i32,local:vec3<f32>,ray:vec3<f32>,scale:f32,medium:i32,prior:GemHit,priorRank:i32)->vec4<f32>{
    let a=gemRow(at);let encoded=gemRow(at+1);var e1=encoded.xyz;var e2=gemRow(at+2).xyz;
    if(encoded.w<0.5){e1-=a.xyz;e2-=a.xyz;}
    let h=cross(ray,e2);let det=dot(e1,h);if(abs(det)<0.000000000001){return vec4<f32>(-1.0);}
    let offset=local-a.xyz;let u=dot(offset,h)/det;if(u<0.0||u>1.0){return vec4<f32>(-1.0);}
    let q=cross(offset,e1);let v=dot(ray,q)/det;if(v<0.0||u+v>1.0){return vec4<f32>(-1.0);}
    let t=dot(e2,q)/det*scale;
    if(t>0.0000001&&(t<prior.distance||(t==prior.distance&&rank<priorRank))){
        let n=gemControlNormal(at,u,v);if(dot(ray,n)>0.0){return vec4<f32>(n,t);}
    }
    return vec4<f32>(-1.0);
}
fn probeFacetExit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32,descriptor:i32)->GemHit{
    let pose=gemRow(192+2*((medium-4)/256));let local=(origin-pose.xyz)/pose.w;
    let inverse=gemInverseRay(ray);let shape=gemRow(descriptor);
    var hit=GemHit(farLimit,vec3<f32>(0.0),-1);var bestRank=2147483647;
    for(var f:i32=0;f<128;f++){
        if(f>=i32(shape.y)){break;}let at=i32(shape.x)+4*f;
        let plane=gemRow(at);let lo=gemRow(at+1);let hi=gemRow(at+2);let info=gemRow(at+3);
        if(!gemBox(local,inverse,(lo.xyz+hi.xyz)*0.5,(hi.xyz-lo.xyz)*0.5,hit.distance/pose.w)){continue;}
        let denom=dot(plane.xyz,ray);let offset=dot(plane.xyz,local);
        if(abs(denom)>0.000000000001){
            let a=(plane.w-offset)/denom;let b=(lo.w-offset)/denom;
            if(max(a,b)*pose.w<=0.0000001||min(a,b)*pose.w>hit.distance+0.0000002){continue;}
        }
        for(var t:i32=0;t<128;t++){
            if(t>=i32(info.x)){break;}let triangleRef=gemRow(i32(hi.w)+t);
            let candidate=probeFacetTriangle(i32(triangleRef.x),i32(triangleRef.y),local,ray,pose.w,medium,hit,bestRank);
            if(candidate.w>0.0){hit=GemHit(candidate.w,candidate.xyz,medium);bestRank=i32(triangleRef.y);}
        }
    }
    return hit;
}
'''

IDEAL_WGSL = r'''
struct ProbeInterval{near:f32,far:f32,entry:vec3<f32>,exit:vec3<f32>,valid:bool};
fn probePlaneInterval(local:vec3<f32>,ray:vec3<f32>,start:i32,count:i32)->ProbeInterval{
    var near=-1e20;var far=1e20;var entry=vec3<f32>(0.0);var exit=vec3<f32>(0.0);
    for(var i:i32=0;i<26;i++){
        if(i>=count){break;}let plane=gemRow(start+2*i);let shading=gemRow(start+2*i+1).xyz;
        let denom=dot(plane.xyz,ray);let distance=plane.w-dot(plane.xyz,local);
        if(abs(denom)<0.000000000001){if(distance<0.0){return ProbeInterval(near,far,entry,exit,false);}}
        else{let t=distance/denom;
            if(denom<0.0&&t>near){near=t;entry=shading;}
            if(denom>0.0&&t<far){far=t;exit=shading;}}
        if(far<near){return ProbeInterval(near,far,entry,exit,false);}
    }
    return ProbeInterval(near,far,entry,exit,true);
}
fn probeConstructionExit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32,descriptor:i32)->GemHit{
    let pose=gemRow(192+2*((medium-4)/256));let local=(origin-pose.xyz)/pose.w;let shape=gemRow(descriptor);
    let miss=GemHit(farLimit,vec3<f32>(0.0),-1);
    let outer=probePlaneInterval(local,ray,i32(shape.x),i32(shape.y));if(!outer.valid){return miss;}
    var distance=outer.far;var normal=outer.exit;
    if(shape.w>0.0){let cut=probePlaneInterval(local,ray,i32(shape.z),i32(shape.w));
        if(cut.valid&&cut.far>outer.near&&cut.near<outer.far){
            let stop=min(outer.far,cut.near);
            if(stop>=outer.near&&stop*pose.w>0.0000001){distance=stop;normal=cut.entry;}
            else if(outer.far>=max(outer.near,cut.far)&&outer.far*pose.w>0.0000001){distance=outer.far;normal=outer.exit;}
            else{return miss;}
        }
    }
    distance*=pose.w;
    if(distance>0.0000001&&distance<farLimit){return GemHit(distance,gemSafeUnit(normal),medium);}
    return miss;
}
'''


def append_exact(rows, groups):
    assert len(groups) <= 128 and max(len(g["triangles"]) for g in groups) <= 128
    descriptor = len(rows)
    rows.append([descriptor+1, len(groups), 0, 0])
    first = len(rows)
    rows.extend([[0.,0.,0.,0.] for _ in range(len(groups)*4)])
    for index, group in enumerate(groups):
        refs = len(rows)
        rows.extend([[t["address"], t["rank"], 0, 0] for t in group["triangles"]])
        at = first + index*4
        rows[at:at+4] = [group["normal"]+[group["lo"]], group["minimum"]+[group["hi"]],
                        group["maximum"]+[refs], [len(group["triangles"]),0,0,0]]
    return descriptor


def append_ideal(rows, shape):
    descriptor = len(rows)
    rows.append([0.,0.,0.,0.])
    outer = len(rows)
    for plane in shape["outer"]: rows.extend([plane["normal"]+[plane["distance"]], plane["shading"]+[0]])
    cutter = len(rows)
    for plane in shape["cutter"]: rows.extend([plane["normal"]+[plane["distance"]], plane["shading"]+[0]])
    rows[descriptor] = [outer, len(shape["outer"]), cutter, len(shape["cutter"])]
    return descriptor


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shader", type=Path, required=True)
    parser.add_argument("--frame", type=Path, required=True)
    parser.add_argument("--asset-root", type=Path, default=ROOT,
                        help="Root containing Art/AudioWorkstation source GLBs (default: repository root)")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mode", choices=["exact-facets", "analytic-construction"], required=True)
    parser.add_argument("--rays", type=int, default=256)
    args = parser.parse_args()
    assert 1 <= args.rays <= 10000
    assert not str(args.output.resolve()).startswith(str(ROOT/"Art"))
    assert not str(args.output.resolve()).startswith(str(ROOT/"Procedures"))
    rows = read_rows(args.frame)
    original_count = len(rows)
    base = int(rows[12][1])
    nodes = source_nodes(args.asset_root)
    report = {"private_candidate": True, "mode": args.mode, "source_shader_sha256": hashlib.sha256(args.shader.read_bytes()).hexdigest(),
              "source_frame_sha256": hashlib.sha256(args.frame.read_bytes()).hexdigest(), "original_rows": original_count,
              "asset_root": str(args.asset_root.resolve()),
              "scope": "Control interior exits only. Exterior queries, materials, lighting, samples and path budgets remain canonical.",
              "shapes": [], "fallback_materials": []}
    dispatch = []
    for material in MATERIALS:
        triangles = original_triangles(rows, base, material)
        if not triangles: continue
        groups = facet_groups(triangles)
        node = nodes.get(material)
        shape = validate_construction(node, triangles) if node else None
        check = comparisons(triangles, groups, shape, args.rays, 137+material)
        info = {"material": material, "name": node["name"] if node else "unknown", "triangles": len(triangles),
                "facet_groups": len(groups), "comparison": check}
        if shape:
            info.update({key: shape[key] for key in ["accepted", "max_triangle_plane_residual_m", "qualification"]})
            info["outer_planes"], info["cutter_planes"] = len(shape["outer"]), len(shape["cutter"])
        if args.mode == "exact-facets": descriptor = append_exact(rows, groups)
        elif shape and shape["accepted"]: descriptor = append_ideal(rows, shape)
        else:
            report["fallback_materials"].append(material)
            report["shapes"].append(info)
            continue
        dispatch.append((material, descriptor))
        report["shapes"].append(info)
    original = args.shader.read_text()
    assert original.count("fn gemControlExit(") == 1
    shader = original.replace("fn gemControlExit(", "fn probeOriginalControlExit(", 1)
    shader += EXACT_WGSL if args.mode == "exact-facets" else IDEAL_WGSL
    shader += "\nfn gemControlExit(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit{\n if((medium-4)/256>=10){return probeOriginalControlExit(origin,ray,medium,farLimit);}\n let material=(medium-4)%256;\n"
    function = "probeFacetExit" if args.mode == "exact-facets" else "probeConstructionExit"
    for material, descriptor in dispatch:
        shader += f" if(material=={material}){{return {function}(origin,ray,medium,farLimit,{descriptor});}}\n"
    shader += " return probeOriginalControlExit(origin,ray,medium,farLimit);\n}\n"
    values = array.array("f", (f32(v) for row in rows for v in row))
    if sys.byteorder != "little": values.byteswap()
    binary = values.tobytes()
    assert binary[:original_count*16] == args.frame.read_bytes(), "Original optical frame changed"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.with_suffix(".wgsl").write_text(shader)
    args.output.with_suffix(".bin").write_bytes(binary)
    report["appended_rows"] = len(rows)-original_count
    report["bytes"] = len(binary)
    report["production_changed"] = False
    args.output.with_suffix(".json").write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps({"candidate": str(args.output), "mode": args.mode, "accelerated_materials": [m for m,_ in dispatch],
                      "fallback_materials": report["fallback_materials"], "appended_rows": report["appended_rows"]}))


if __name__ == "__main__": main()
