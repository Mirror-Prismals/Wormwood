"""Pack the existing InputMonitor GLBs for the workstation optical renderer.

No Blender process or geometry generation is involved. The three exports have
identical geometry and differ only in material state. Retain that geometry once,
with three palettes, after the existing immutable engraving-density packet.
Run this script explicitly to write InputMonitor/runtime.{bin,json}.

Triangles use the existing six-row float32 edge/normal encoding. A stackless
12-bin surface-area BVH has at most four triangles per leaf. The manifest's
node_count is a conservative traversal limit; never use the older 4096-node
control-loop limit for this substantially more detailed authored assembly.
"""

import hashlib
import json
import math
import struct
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "Art/AudioWorkstation/InputMonitor"
MODES = ("Off", "Auto", "In")
FRAME_ROWS = 256 + 3 * 64 * 64 + 32768 + 4096
LUT_ROWS = 57 ** 3
DENSITY_ROWS = 95 * 21846 // 2
BASE = FRAME_ROWS + LUT_ROWS + DENSITY_ROWS
assert (FRAME_ROWS, DENSITY_ROWS, BASE) == (49408, 1037685, 1272286)
CAPACITY = 1_310_720
MAX_NODES = 131072
MATERIAL_STRIDE = 40
MATERIAL_COUNT = 39
BIN_COUNT = 12
LEAF_SIZE = 4
MAX_DEPTH = 48
IDENTITY = tuple(tuple(float(i == j) for j in range(4)) for i in range(4))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def read_glb(path):
    raw = path.read_bytes()
    require(len(raw) >= 20, f"{path.name}: truncated GLB")
    magic, version, length = struct.unpack_from("<III", raw)
    require((magic, version, length) == (0x46546C67, 2, len(raw)), "invalid GLB header")
    chunks = {}
    offset = 12
    while offset < len(raw):
        size, kind = struct.unpack_from("<II", raw, offset)
        offset += 8
        require(offset + size <= len(raw) and kind not in chunks, "invalid GLB chunk")
        chunks[kind] = raw[offset:offset + size]
        offset += size
    require(0x4E4F534A in chunks and 0x004E4942 in chunks, "GLB needs JSON and BIN chunks")
    return json.loads(chunks[0x4E4F534A]), chunks[0x004E4942], hashlib.sha256(raw).hexdigest()


def material_role(material, mode):
    role = material.get("extras", {}).get("role")
    if role:
        return role
    name = material["name"]
    return name.removeprefix("InputMonitor_" + mode + "_")


def geometry_signature(document, roles):
    """Names/extras differ by state; topology, transforms and material roles do not."""
    nodes = [{k: v for k, v in node.items() if k not in ("name", "extras")}
             for node in document["nodes"]]
    meshes = []
    for mesh in document["meshes"]:
        primitives = []
        for primitive in mesh["primitives"]:
            primitive = dict(primitive)
            primitive["material"] = roles[primitive["material"]]
            primitives.append(primitive)
        meshes.append(primitives)
    scenes = [{k: v for k, v in scene.items() if k not in ("name", "extras")}
              for scene in document["scenes"]]
    return (document["accessors"], document["bufferViews"], nodes, meshes,
            scenes, document.get("scene", 0))


def material_rows(material):
    pbr = material.get("pbrMetallicRoughness", {})
    extensions = material.get("extensions", {})
    require(material.get("alphaMode", "OPAQUE") == "OPAQUE", "unsupported monitor alpha mode")
    require(not any("Texture" in key for key in pbr), "monitor material texture unsupported")
    transmission = extensions.get("KHR_materials_transmission", {}).get("transmissionFactor", 0)
    require(transmission == 0, "monitor geometry currently requires opaque materials")
    coat = extensions.get("KHR_materials_clearcoat", {})
    emission_strength = extensions.get("KHR_materials_emissive_strength", {}).get("emissiveStrength", 1)
    emission = [c * emission_strength for c in material.get("emissiveFactor", [0, 0, 0])]
    rows = [[0.0] * 4 for _ in range(MATERIAL_STRIDE)]
    rows[0] = [1, extensions.get("KHR_materials_ior", {}).get("ior", 1.5),
               pbr.get("roughnessFactor", 1), transmission]
    rows[2][3] = coat.get("clearcoatFactor", 0)
    rows[3] = [.001, .001, .001, coat.get("clearcoatRoughnessFactor", 0)]
    rows[4] = pbr.get("baseColorFactor", [1, 1, 1, 1])
    rows[5] = emission + [pbr.get("metallicFactor", 1)]
    # Opaque surfaces do not consume the dielectric energy table, but populate
    # its neutral values to keep the existing 40-row material ABI complete.
    for i in range(32, 40):
        rows[i] = [1.0] * 4
    return rows


def multiply(a, b):
    return tuple(tuple(sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4))
                 for i in range(4))


def transform(node):
    if "matrix" in node:
        return tuple(tuple(node["matrix"][4 * j + i] for j in range(4)) for i in range(4))
    x, y, z, w = node.get("rotation", (0, 0, 0, 1))
    scale = node.get("scale", (1, 1, 1))
    t = node.get("translation", (0, 0, 0))
    m = [[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w), t[0]],
         [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w), t[1]],
         [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y), t[2]], [0, 0, 0, 1]]
    for i in range(3):
        for j in range(3):
            m[i][j] *= scale[j]
    return m


def vector(matrix, value, normal=False):
    v = [sum(matrix[i][j] * value[j] for j in range(3))
         + (0 if normal else matrix[i][3]) for i in range(3)]
    # Undo Blender's standard glTF axis conversion: authored +Y up, +Z outward.
    return (f32(v[0]), f32(-v[2]), f32(v[1]))


def unpack_triangles(document, binary, expected_counts=(27, 128450), root_transform=IDENTITY):
    def accessor(index):
        a = document["accessors"][index]
        view = document["bufferViews"][a["bufferView"]]
        require(not a.get("sparse") and view.get("buffer", 0) == 0, "unsupported accessor")
        fmt = "<" + {5126: "f", 5125: "I", 5123: "H", 5121: "B"}[a["componentType"]] * {
            "SCALAR": 1, "VEC3": 3}[a["type"]]
        stride = view.get("byteStride", struct.calcsize(fmt))
        offset = view.get("byteOffset", 0) + a.get("byteOffset", 0)
        return [struct.unpack_from(fmt, binary, offset + i * stride) for i in range(a["count"])]

    def posed(value):
        # Pose in renderer coordinates, after Blender's axis conversion.
        return tuple(f32(sum(root_transform[i][j] * value[j] for j in range(3)))
                     for i in range(3))

    triangles = []
    components = []

    def visit(index, parent):
        node = document["nodes"][index]
        require("skin" not in node, "skinned monitor mesh unsupported")
        matrix = multiply(parent, transform(node))
        require(all(abs(sum(matrix[k][i]*matrix[k][j] for k in range(3)) - (i == j)) < 1e-5
                    for i in range(3) for j in range(3)), "monitor transforms must be rigid")
        if "mesh" in node:
            first = len(triangles)
            materials = set()
            for primitive in document["meshes"][node["mesh"]]["primitives"]:
                require(primitive.get("mode", 4) == 4 and "targets" not in primitive,
                        "monitor primitive must contain static triangles")
                positions = [posed(vector(matrix, v)) for v in accessor(primitive["attributes"]["POSITION"])]
                normals = [posed(vector(matrix, v, True)) for v in accessor(primitive["attributes"]["NORMAL"])]
                indices = [v[0] for v in accessor(primitive["indices"])]
                require(len(indices) % 3 == 0, "incomplete monitor triangle")
                material = primitive["material"]
                materials.add(material)
                for start in range(0, len(indices), 3):
                    ids = indices[start:start + 3]
                    vertices = tuple(positions[j] for j in ids)
                    ns = tuple(normals[j] for j in ids)
                    lo = tuple(min(v[axis] for v in vertices) for axis in range(3))
                    hi = tuple(max(v[axis] for v in vertices) for axis in range(3))
                    center = tuple(sum(v[axis] for v in vertices) / 3 for axis in range(3))
                    triangles.append((vertices, ns, material, lo, hi, center))
            components.append({"name": node.get("name", "unnamed"), "first_source_triangle": first,
                               "triangles": len(triangles) - first, "materials": sorted(materials)})
        for child in node.get("children", []):
            visit(child, matrix)

    for root in document["scenes"][document.get("scene", 0)]["nodes"]:
        visit(root, IDENTITY)
    if expected_counts is not None:
        require((len(components), len(triangles)) == expected_counts, "authored geometry count changed")
    return triangles, components


def area(lo, hi):
    x, y, z = (max(0, hi[i] - lo[i]) for i in range(3))
    return 2 * (x*y + x*z + y*z)


def pack_bvh(triangles, rows, base=BASE):
    nodes = []
    ordered = []
    deepest = 0

    def build(ids, depth):
        nonlocal deepest
        deepest = max(deepest, depth)
        index = len(nodes)
        nodes.append(None)
        lo = tuple(min(triangles[t][3][axis] for t in ids) for axis in range(3))
        hi = tuple(max(triangles[t][4][axis] for t in ids) for axis in range(3))
        first = len(ordered)
        count = len(ids) if len(ids) <= LEAF_SIZE else 0
        if count:
            ordered.extend(ids)
        else:
            best = None
            if depth < MAX_DEPTH:
                for axis in range(3):
                    lower = min(triangles[t][5][axis] for t in ids)
                    upper = max(triangles[t][5][axis] for t in ids)
                    if upper - lower < 1e-12:
                        continue
                    inverse = BIN_COUNT / (upper - lower)
                    bins = [[0, [math.inf]*3, [-math.inf]*3] for _ in range(BIN_COUNT)]
                    for t in ids:
                        triangle = triangles[t]
                        bucket = min(BIN_COUNT-1, int((triangle[5][axis]-lower)*inverse))
                        entry = bins[bucket]
                        entry[0] += 1
                        for k in range(3):
                            entry[1][k] = min(entry[1][k], triangle[3][k])
                            entry[2][k] = max(entry[2][k], triangle[4][k])
                    prefix = []
                    size, a, b = 0, [math.inf]*3, [-math.inf]*3
                    for n, low, high in bins:
                        size += n
                        a = [min(a[k], low[k]) for k in range(3)]
                        b = [max(b[k], high[k]) for k in range(3)]
                        prefix.append((size, area(a, b) if size else 0))
                    size, a, b = 0, [math.inf]*3, [-math.inf]*3
                    for cut in range(BIN_COUNT-1, 0, -1):
                        n, low, high = bins[cut]
                        size += n
                        a = [min(a[k], low[k]) for k in range(3)]
                        b = [max(b[k], high[k]) for k in range(3)]
                        left_count, left_area = prefix[cut-1]
                        if not size or not left_count:
                            continue
                        cost = left_count * left_area + size * area(a, b)
                        if best is None or cost < best[0]:
                            best = (cost, axis, lower, inverse, cut)
            if best is None:
                axis = max(range(3), key=lambda i: hi[i]-lo[i])
                ids.sort(key=lambda t: triangles[t][5][axis])
                half = len(ids) // 2
                left, right = ids[:half], ids[half:]
            else:
                _, axis, lower, inverse, cut = best
                left, right = [], []
                for t in ids:
                    bucket = min(BIN_COUNT-1, int((triangles[t][5][axis]-lower)*inverse))
                    (left if bucket < cut else right).append(t)
            require(left and right, "empty monitor BVH child")
            build(left, depth + 1)
            build(right, depth + 1)
        nodes[index] = (lo, hi, len(nodes), first, count,
                        min(triangles[t][2] for t in ids), max(triangles[t][2] for t in ids))

    build(list(range(len(triangles))), 0)
    root = base + len(rows)
    end = root + 3 * len(nodes)
    for lo, hi, escape, first, count, material_min, material_max in nodes:
        rows.extend(([*lo, root + escape*3], [*hi, end + first*6],
                     [count, material_min, material_max, 0]))
    for index in ordered:
        vertices, normals, material, *_ = triangles[index]
        edges = [tuple(f32(v[i] - vertices[0][i]) for i in range(3)) for v in vertices[1:]]
        flat = normals[0] == normals[1] == normals[2]
        rows.extend(([*vertices[0], material], [*edges[0], 1], [*edges[1], 0],
                     [*normals[0], int(flat)], [*normals[1], 0], [*normals[2], 0]))
    require(len(ordered) == len(triangles) and len(set(ordered)) == len(triangles),
            "monitor BVH must retain every source triangle exactly once")
    return {"root": root, "end": end, "min": nodes[0][0], "max": nodes[0][1],
            "node_count": len(nodes), "max_depth": deepest, "leaf_size": LEAF_SIZE,
            "triangles": len(triangles), "triangle_rows": len(triangles)*6,
            "traversal_limit": len(nodes)}


def main():
    documents = []
    sources = {}
    canonical_roles = None
    reference_signature = None
    reference_binary = None
    rows = []
    palettes = []
    for mode, name in enumerate(MODES):
        filename = "InputMonitor_" + name + ".glb"
        document, binary, digest = read_glb(ART / filename)
        roles = [material_role(m, name) for m in document["materials"]]
        require(len(roles) == MATERIAL_COUNT and len(set(roles)) == MATERIAL_COUNT,
                "monitor requires 39 unique semantic material roles")
        signature = geometry_signature(document, roles)
        if mode == 0:
            canonical_roles, reference_signature, reference_binary = roles, signature, binary
        require(roles == canonical_roles and signature == reference_signature and binary == reference_binary,
                "monitor states must share identical geometry and material-role indices")
        selected = []
        base = BASE + len(rows)
        for index, material in enumerate(document["materials"]):
            packed = material_rows(material)
            emitting = any(c > 0 for c in packed[5][:3])
            role = roles[index]
            lamp_mode = material.get("extras", {}).get("lamp_mode")
            if lamp_mode is not None and "Diode" in role:
                require(emitting == (lamp_mode == mode), "monitor state lights the wrong diode group")
            if emitting:
                selected.append(index)
            rows.extend(packed)
        require(len(selected) == 8, "each selected monitor lamp must have eight emitting materials")
        palettes.append({"mode": mode, "name": name, "material_base": base,
                         "materials": MATERIAL_COUNT, "emitting_materials": selected,
                         "emitting_roles": [roles[i] for i in selected]})
        sources[filename] = digest
        documents.append(document)
    # Cardinal's screen projection exposes the left side and looks down into
    # the hoods. Use a rigid yaw/pitch approximation, preserving real depth and
    # rotating normals with the housing. Other exporters retain identity poses.
    yaw, pitch = math.radians(-37), math.radians(-12)
    cy, sy, cp, sp = math.cos(yaw), math.sin(yaw), math.cos(pitch), math.sin(pitch)
    pose = ((cy, 0., sy, 0.), (sp*sy, cp, -sp*cy, 0.),
            (-cp*sy, sp, cp*cy, 0.), (0., 0., 0., 1.))
    triangles, components = unpack_triangles(documents[0], reference_binary, root_transform=pose)
    geometry = pack_bvh(triangles, rows)
    require(len(rows) <= CAPACITY, f"monitor packet exceeds {CAPACITY} rows: {len(rows)}")
    require(geometry["node_count"] <= MAX_NODES,
            f"monitor BVH exceeds shader traversal bound: {geometry['node_count']}")
    require(BASE + len(rows) < 2**24, "absolute row indices must be exact float32 integers")
    require(all(len(row) == 4 and all(math.isfinite(v) for v in row) for row in rows),
            "nonfinite monitor packet")
    manifest = {"schema": 1, "base": BASE, "capacity": CAPACITY, "rows": len(rows),
                "material_stride": MATERIAL_STRIDE, "material_count": MATERIAL_COUNT,
                "material_bases": [palette["material_base"] for palette in palettes],
                "root": geometry["root"], "end": geometry["end"],
                "min": geometry["min"], "max": geometry["max"],
                "triangles": geometry["triangles"], "nodes": geometry["node_count"],
                "palettes": palettes, "material_roles": canonical_roles,
                "geometry": geometry, "components": components, "source_sha256": sources,
                "geometry_binary_sha256": hashlib.sha256(reference_binary).hexdigest(),
                "triangle_encoding": "flagged_float32_edges_and_flat_normals",
                "coordinates": "+X right, +Y up, +Z outward; metres; front housing centre origin",
                "pose_degrees": {"yaw": -37, "pitch": -12, "order": "yaw_then_pitch"},
                "lamp_centres_m": [tuple(sum(pose[i][j]*p[j] for j in range(3)) for i in range(3)) for p in
                                   ((0, .053, .003), (0, -.002, .003), (0, -.057, .003))],
                "lamp_radius_m": .02235,
                "material_extension_words": {"metallic": "row+5.w", "clearcoat": "row+2.w",
                                             "clearcoat_roughness": "row+3.w"},
                "notes": ["Shared authored geometry is uploaded once; palette selection is per instance.",
                          "Emission RGB includes KHR_materials_emissive_strength.",
                          "Native opaque shading consumes the authored metal/coat weights using bounded direct GGX lighting; no Cycles parity claim.",
                          "Whole-tree traversal must allow geometry.traversal_limit nodes; the old 4096 bound is insufficient."]}
    blob = b"".join(struct.pack("<4f", *row) for row in rows)
    manifest["runtime_sha256"] = hashlib.sha256(blob).hexdigest()
    # Both results are completely prepared before replacing either existing file.
    (ART / "runtime.bin").write_bytes(blob)
    (ART / "runtime.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"base": BASE, "rows": len(rows), "bytes": len(blob), **geometry}))


if __name__ == "__main__":
    main()
