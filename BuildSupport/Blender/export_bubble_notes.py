"""Export the editable Short bubble without saving or rendering its .blend.

Run Blender --background Art/AudioWorkstation/BubbleNotes/BubbleNotes.blend
    --python BuildSupport/Blender/export_bubble_notes.py

The original mesh, triangulation, smooth normals and both shape-key deltas are
retained. A validated surface descriptor lets native code change note duration
without stretching the rounded shoulders. The Short mesh's 24 rings and boundary
sample count remain fixed at every duration; long notes do not gain triangles.
No Blender material nodes, procedural textures, images or preview scene objects
are exported. The native thin-film material is a separate presentation contract.
"""
import hashlib
import json
import math
import struct
from pathlib import Path

import bpy


ROOT = Path(__file__).resolve().parents[2]
DIRECTORY = ROOT / "Art/AudioWorkstation/BubbleNotes"
SOURCE = DIRECTORY / "BubbleNotes.blend"
LENGTH, HEIGHT, DEPTH = .0225, .018, .0108
RINGS = 24
VERTEX_STRIDE = 6
NODE_STRIDE = 4
# Mirrors the reserved immutable scene blocks, including the existing AgX LUT
# and engraving density data. Absolute row addresses remain exactly f32 integers.
ENGRAVING_BASE = 256 + 3 * 64 * 64 + 32768
INPUT_BASE = ENGRAVING_BASE + 4096 + 57 ** 3 + 95 * 21846 // 2
EDISON_BASE = INPUT_BASE + 1310720
BASE = (EDISON_BASE + 32768 + 255) // 256 * 256
CAPACITY = 32768


def boundary_descriptors():
    """The source generator's sampling, expressed as coefficients of L,H,r.

    Each entry is (xL,xR,yH,yR,tangentX,tangentY). It is independently checked
    against the actual .blend vertices below, rather than assuming source code
    still describes the user's asset.
    """
    radius = min(LENGTH * .46, HEIGHT * .28)
    signs = ((1, 1), (-1, 1), (-1, -1), (1, -1))
    result = []
    for corner, (sx, sy) in enumerate(signs):
        for sample in range(13):
            angle = (corner + sample / 12) * math.pi / 2
            result.append((sx * .5, -sx + math.cos(angle),
                           sy * .5, -sy + math.sin(angle),
                           -math.sin(angle), math.cos(angle)))
        end = result[-1]
        nx, ny = signs[(corner + 1) % 4]
        angle = (corner + 1) * math.pi / 2
        start = (nx * .5, -nx + math.cos(angle),
                 ny * .5, -ny + math.sin(angle))
        a = (end[0] * LENGTH + end[1] * radius,
             end[2] * HEIGHT + end[3] * radius)
        b = (start[0] * LENGTH + start[1] * radius,
             start[2] * HEIGHT + start[3] * radius)
        distance = math.dist(a, b)
        count = max(1, math.ceil(distance / (HEIGHT / 6)))
        for sample in range(1, count):
            t = sample / count
            coefficients = tuple(end[i] + (start[i] - end[i]) * t for i in range(4))
            result.append(coefficients + ((b[0] - a[0]) / distance,
                                           (b[1] - a[1]) / distance))
    return result


def surface_descriptors():
    boundary = boundary_descriptors()
    # Each item has coordinate coefficients, ring coordinates, and the original
    # boundary tangent. Pole flags avoid dividing by sin(phi) at the two tips.
    result = [([.5, 0, 0, 0], [.6, 0, 1, 1], [0, 0])]
    for ring in range(1, RINGS):
        phi = math.pi * ring / RINGS
        sine, cosine = math.sin(phi), math.cos(phi)
        for x_l, x_r, y_h, y_r, tx, ty in boundary:
            result.append(([.5 + sine * x_l, sine * x_r,
                            sine * y_h, sine * y_r],
                           [.3 * (1 + cosine), sine, cosine, 0], [tx, ty]))
    result.append(([.5, 0, 0, 0], [0, 0, -1, -1], [0, 0]))
    return result, len(boundary)


def rest_position(descriptor, length=LENGTH, height=HEIGHT):
    c, ring, _ = descriptor
    radius = min(length * .46, height * .28)
    return (c[0] * length + c[1] * radius,
            c[2] * height + c[3] * radius, ring[0] * height)


def shape_deltas(position):
    x, y, z = position
    u, v = x / LENGTH, y / (HEIGHT * .5)
    ends = math.sin(math.pi * u) ** 2
    wave = math.sin(math.tau * u)
    squash = (0, .10 * y * (1 - v * v) * ends, -.32 * z)
    settling = (0, .00075 * wave * (1 - v * v) * ends,
                .00135 * wave * ends * z / DEPTH)
    return squash, settling


def export():
    if Path(bpy.data.filepath).resolve() != SOURCE.resolve():
        raise RuntimeError("Open the authoritative BubbleNotes.blend before exporting")
    original_hash = hashlib.sha256(SOURCE.read_bytes()).hexdigest()
    obj = bpy.data.objects.get("BubbleNote_Short")
    if obj is None or obj.type != "MESH" or len(obj.modifiers):
        raise RuntimeError("Expected the unmodified authored BubbleNote_Short mesh")
    mesh = obj.data
    mesh.calc_loop_triangles()
    descriptors, around = surface_descriptors()
    if len(mesh.vertices) != len(descriptors):
        raise RuntimeError("Bubble topology changed; revise the descriptor exporter")
    keys = mesh.shape_keys.key_blocks if mesh.shape_keys else None
    if keys is None or any(name not in keys for name in
                           ("Rest", "Plop_Squash_Stretch", "Settling_Surface_Wave")):
        raise RuntimeError("Both authored placement shape keys are required")
    if any(not face.use_smooth for face in mesh.polygons):
        raise RuntimeError("Bubble source must retain its smooth membrane normals")
    vertices = []
    rest_error = 0.
    shape_error = 0.
    for index, (vertex, descriptor) in enumerate(zip(mesh.vertices, descriptors)):
        position = tuple(vertex.co)
        rest_error = max(rest_error, math.dist(position, rest_position(descriptor)),
                         math.dist(position, keys["Rest"].data[index].co))
        deltas = []
        for name, expected in zip(("Plop_Squash_Stretch", "Settling_Surface_Wave"),
                                  shape_deltas(position)):
            delta = tuple(keys[name].data[index].co[k] - position[k] for k in range(3))
            shape_error = max(shape_error, math.dist(delta, expected))
            deltas.append(delta)
        c, ring, tangent = descriptor
        vertices.extend((c, ring, tangent + [position[0], position[1]],
                         [position[2], *vertex.normal],
                         [*deltas[0], 0], [*deltas[1], 0]))
    if rest_error > 1e-8 or shape_error > 1e-8:
        raise RuntimeError(f"Authored membrane no longer matches descriptors: {rest_error}, {shape_error}")
    triangles = [tuple(t.vertices) for t in mesh.loop_triangles]
    if len(triangles) != 2944 or len(descriptors) != 1474:
        raise RuntimeError("Unexpected Short bubble topology")
    # Also reject changed winding or a damaged boundary. This operates on the
    # original indexed mesh; no decimation or replacement geometry is performed.
    edges = {}
    volume = 0.
    for triangle in triangles:
        a, b, c = (mesh.vertices[index].co for index in triangle)
        if (b - a).cross(c - a).length < 1e-13:
            raise RuntimeError("Degenerate authored triangle")
        volume += a.dot(b.cross(c)) / 6
        for i in range(3):
            first, second = triangle[i], triangle[(i + 1) % 3]
            key = tuple(sorted((first, second)))
            counts = edges.setdefault(key, [0, 0])
            counts[int(first > second)] += 1
    if volume <= 0 or any(count != [1, 1] for count in edges.values()):
        raise RuntimeError("The authored membrane must be closed with positive volume")

    nodes, ordered = [], []

    def build(source_triangles):
        node_index = len(nodes)
        nodes.append(None)
        indices = sorted({i for triangle in source_triangles for i in triangle})
        coefficient_min = [min(descriptors[i][0][k] for i in indices) for k in range(4)]
        coefficient_max = [max(descriptors[i][0][k] for i in indices) for k in range(4)]
        zmin = min(descriptors[i][1][0] for i in indices)
        zmax = max(descriptors[i][1][0] for i in indices)
        first, count = len(ordered), 0
        if len(source_triangles) <= 4:
            ordered.extend(source_triangles)
            count = len(source_triangles)
        else:
            centers = [tuple(sum(mesh.vertices[i].co[k] for i in triangle) / 3
                             for k in range(3)) for triangle in source_triangles]
            axis = max(range(3), key=lambda k: max(c[k] for c in centers) - min(c[k] for c in centers))
            ranked = sorted(zip(source_triangles, centers), key=lambda item: item[1][axis])
            middle = len(ranked) // 2
            build([item[0] for item in ranked[:middle]])
            build([item[0] for item in ranked[middle:]])
        nodes[node_index] = (coefficient_min, coefficient_max,
                             zmin, zmax, len(nodes), first, count)

    build(triangles)
    node_end = BASE + len(nodes) * NODE_STRIDE
    vertex_base = node_end
    triangle_base = vertex_base + len(vertices)
    rows = []
    for cmin, cmax, zmin, zmax, escape, first, count in nodes:
        rows.extend((cmin, cmax,
                     [zmin, zmax, BASE + escape * NODE_STRIDE, triangle_base + first],
                     [count, 0, 0, 0]))
    rows.extend(vertices)
    rows.extend([*triangle, 0] for triangle in ordered)
    if len(rows) > CAPACITY or len(nodes) > 2048:
        raise RuntimeError("Bubble packet exceeds its bounded native capacity")
    if any(len(row) != 4 or any(not math.isfinite(x) for x in row) for row in rows):
        raise RuntimeError("Invalid bubble packet row")
    document = {
        "schema": 1, "base": BASE, "capacity": CAPACITY, "rows": len(rows),
        "root": BASE, "end": node_end, "node_count": len(nodes),
        "vertex_base": vertex_base, "vertex_count": len(descriptors),
        "vertex_stride": VERTEX_STRIDE, "triangle_base": triangle_base,
        "triangle_count": len(ordered), "node_stride": NODE_STRIDE,
        "source": "BubbleNotes.blend", "source_object": obj.name,
        "source_sha256": original_hash,
        "source_dimensions_m": [LENGTH, HEIGHT, DEPTH],
        "source_rest_max_error_m": rest_error, "source_shape_max_error_m": shape_error,
        "coordinate_system": "metres; onset origin; +X time, +Y pitch, +Z outward",
        "geometry": {"rings": RINGS, "boundary_samples": around,
                     "shoulder_radius": "min(length*.46,height*.28)",
                     "duration_remap": "x=cL*length+cr*radius; y=cH*height+cr*radius; z=cH*height",
                     "fixed_topology": True,
                     "normals": "Analytic authored surface tangents transformed by the exact placement Jacobian; source Blender normals also retained",
                     "limitation": "Long durations keep Short topology; no automatic longitudinal subdivision"},
        "placement": {"settle_seconds": 1.35, "drop_seconds": .125,
                      "drop_height_ratio": .5,
                      "squash": "exp(-6*tau)*cos(20*tau)",
                      "wave": "exp(-4.9*tau)*sin(25*tau)", "tau": "age-.125",
                      "source_shape_keys": ["Plop_Squash_Stretch", "Settling_Surface_Wave"]},
        "material": {"kind": "native_air_film_air", "air_ior": 1., "film_ior": 1.333,
                     "roughness": .012, "thickness_nm": 450., "bulk_absorption": 0.,
                     "procedural_textures": False, "textures": [],
                     "limitation": "Uniform native 450nm film deliberately replaces Blender's noise-driven thickness; not Cycles parity"},
        "format": "little-endian float32 vec4 rows; node stride4; vertex stride6; indexed triangle stride1",
    }
    binary = b"".join(struct.pack("<4f", *row) for row in rows)
    (DIRECTORY / "BubbleNotes_runtime.bin").write_bytes(binary)
    (DIRECTORY / "BubbleNotes_runtime.json").write_text(json.dumps(document, indent=2) + "\n")
    if hashlib.sha256(SOURCE.read_bytes()).hexdigest() != original_hash:
        raise RuntimeError("Authoritative .blend changed during export")
    print(f"BUBBLE_NOTES_EXPORTED: {len(descriptors)} vertices, {len(triangles)} triangles, {len(rows)} rows; source unchanged")


if __name__ == "__main__":
    export()
