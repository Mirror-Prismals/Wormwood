"""Author actual subsurface dot text and the matching native glyph lattice.

Blender --background --factory-startup --python BuildSupport/Blender/make_laser_engraving.py

No render or original-asset modification. The saved copy contains this script as
a Text block: edit SSLE_SOURCE_* FONT bodies, then Run Script to regenerate the
fracture geometry and native packet. Nothing runs automatically on file open.
"""
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import struct

import bpy
import numpy as np
from mathutils import Vector


def repository_root():
    remembered = bpy.context.scene.get("mirror_ssle_repository")
    if remembered and (Path(remembered) / "BuildSupport/Blender").is_dir():
        return Path(remembered)
    return Path(__file__).resolve().parents[2]


ROOT = repository_root()
SOURCE = ROOT / "Art/AudioWorkstation/GemPanels"
OUT = ROOT / "Art/AudioWorkstation/LaserEngraving"
FONT_PATH = ROOT.parent / "Salamander137/Procedures/Fonts/AlegreyaSans-Regular.ttf"
FIRST, COUNT, WIDTH, HEIGHT = 32, 95, 96, 96
PITCH, ORIGIN = .0125, (-.12, -.24)
DENSITY_EDGE, DENSITY_LEVELS, DENSITY_TEXELS_PER_GLYPH = 128, 8, 21846
STYLE = {
    "layer_offsets_em": [0., .07, .14],
    "radius_em": .00375,
    "radius_depth_ratio": .45,
    "layer_shift_cells": [[0., 0.], [.7, -3.6], [1.7, -9.2]],
    "layer_weights": [1., .25, .08],
    "point_jitter_cells": .24,
    "point_radius_range": [.6, 1.4],
    "opacity": .55,
    "base_color_linear_rgb": [.96, .97, 1.],
    "scatter_gain": 2.4,
    "stroke_inset_em": .008,
    "front_depth_minor_fraction": .04,
}
METRES_PER_PX = 1.3 / 720
BLOCKS = [
    {"panel_index": 2, "panel_name": "GemPanel_Tall", "margin_px": 9.6,
     "lines": [
         {"text": "LIBRARY", "font_size_px": 20.4, "baseline_px": [0., 20.4]},
         {"text": "Sounds", "font_size_px": 15.6, "baseline_px": [0., 55.2]},
         {"text": "AUDIO", "font_size_px": 14.4, "baseline_px": [0., 91.2]},
         {"text": "Glass Bell.wav", "font_size_px": 16.8, "baseline_px": [0., 134.4]},
         {"text": "Forest Rain.wav", "font_size_px": 16.8, "baseline_px": [0., 177.6]},
         {"text": "Soft Pulse.wav", "font_size_px": 16.8, "baseline_px": [0., 220.8]},
     ]},
    {"panel_index": 3, "panel_name": "GemPanel_Compact", "margin_px": 9.6,
     "lines": [
         {"text": "SETTINGS", "font_size_px": 21.6, "baseline_px": [0., 21.6]},
         {"text": "LOAD", "font_size_px": 21.6, "baseline_px": [0., 86.4]},
         {"text": "SAVE", "font_size_px": 21.6, "baseline_px": [0., 151.2]},
         {"text": "ORIGAMI", "font_size_px": 21.6, "baseline_px": [0., 216.]},
     ]},
]


def font_curve(name, text, font, inset=True):
    data = bpy.data.curves.new(name, "FONT")
    data.body, data.font, data.size = text, font, 1.
    data.align_x, data.align_y = "LEFT", "TOP_BASELINE"
    data.resolution_u = 24
    data.fill_mode = "BOTH"
    data.extrude, data.bevel_depth = 0., 0.
    # Curve offset is a distance in the generated object's local coordinates.
    # Sample at size 1; hidden editable sources rescale this distance with size.
    data.offset = -STYLE["stroke_inset_em"] if inset else 0.
    return data


def triangulated_text(text, font, inset=True):
    curve = font_curve("SSLE metric source", text, font, inset=inset)
    obj = bpy.data.objects.new("SSLE metric source", curve)
    bpy.context.scene.collection.objects.link(obj)
    bpy.context.view_layer.update()
    evaluated = obj.evaluated_get(bpy.context.evaluated_depsgraph_get())
    mesh = evaluated.to_mesh()
    mesh.calc_loop_triangles()
    vertices = [(v.co.x, v.co.y) for v in mesh.vertices]
    triangles = [tuple(vertices[i] for i in tri.vertices) for tri in mesh.loop_triangles]
    evaluated.to_mesh_clear()
    bpy.data.objects.remove(obj, do_unlink=True)
    bpy.data.curves.remove(curve)
    return vertices, triangles


def contains(point, triangle):
    (ax, ay), (bx, by), (cx, cy) = triangle
    px, py = point
    ab = (bx - ax) * (py - ay) - (by - ay) * (px - ax)
    bc = (cx - bx) * (py - by) - (cy - by) * (px - bx)
    ca = (ax - cx) * (py - cy) - (ay - cy) * (px - cx)
    return min(ab, bc, ca) >= -1e-10 or max(ab, bc, ca) <= 1e-10


def make_glyphs(font):
    """Sample the filled, triangulated Blender glyph; preserve counters/holes."""
    def visual_width(text):
        # Preserve original Alegreya spacing; only the rendered stroke narrows.
        vertices, _ = triangulated_text(text, font, inset=False)
        return max(v[0] for v in vertices) - min(v[0] for v in vertices)

    reference_width = visual_width("AA")
    glyphs, points, packet = [], {}, []
    for cp in range(FIRST, FIRST + COUNT):
        char = chr(cp)
        vertices, triangles = triangulated_text(char, font)
        triangles = [t for t in triangles if abs(
            (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) -
            (t[1][1] - t[0][1]) * (t[2][0] - t[0][0])) > 1e-12]
        advance = visual_width("A" + char + "A") - reference_width
        assert math.isfinite(advance) and advance > 0, (cp, advance)
        cells, words = [], [0] * (WIDTH * HEIGHT // 16)
        for y in range(HEIGHT):
            for x in range(WIDTH):
                p = (ORIGIN[0] + (x + .5) * PITCH,
                     ORIGIN[1] + (y + .5) * PITCH)
                if any(contains(p, tri) for tri in triangles):
                    index = y * WIDTH + x
                    words[index // 16] |= 1 << (index % 16)
                    cells.append(p)
        if cp != 32:
            assert cells, "Empty sampled glyph: " + repr(char)
            # Reject silently clipped outlines instead of exporting broken text.
            assert all(ORIGIN[0] <= x <= ORIGIN[0] + WIDTH * PITCH and
                       ORIGIN[1] <= y <= ORIGIN[1] + HEIGHT * PITCH
                       for x, y in vertices), (char, "font exceeds lattice")
        else:
            assert not cells
        bounds = ([min(x for x, _ in cells), min(y for _, y in cells),
                   max(x for x, _ in cells), max(y for _, y in cells)] if cells else None)
        glyphs.append({"codepoint": cp, "advance_em": advance,
                       "occupied_cells": len(cells), "bounds_em": bounds})
        points[cp] = cells
        packet.extend(float(word) for word in words)
    assert points[ord("A")] and not points[32]
    assert len(packet) == COUNT * 144 * 4
    return glyphs, points, struct.pack("<" + "f" * len(packet), *packet)


def fracture_hash(x, y, glyph_index, layer, salt):
    """Integer-only seed shared with native; independent of time or viewport."""
    n = (x * 73 + y * 151 + glyph_index * 199 + layer * 283 + salt * 317) % 997
    return ((n * n * 157 + n * 313) % 1009) / 1009.


def fracture_sample(x, y, glyph_index, layer):
    """The shared unshifted fracture centre, radius, and glint seed in em."""
    ix = int(round((x - ORIGIN[0]) / PITCH - .5))
    iy = int(round((y - ORIGIN[1]) / PITCH - .5))
    def noise(salt):
        return fracture_hash(ix, iy, glyph_index, layer, salt)
    jitter_x = (noise(1) - .5) * 2 * STYLE["point_jitter_cells"] * PITCH
    jitter_y = (noise(2) - .5) * 2 * STYLE["point_jitter_cells"] * PITCH
    low, high = STYLE["point_radius_range"]
    radius = STYLE["radius_em"] * (low + (high - low) * noise(3))
    return x + jitter_x, y + jitter_y, radius, noise(4)


def make_density_packet(points):
    """Bake the same three interior fields with area integration and float mips.

    Each separable Gaussian is integrated over its intersecting texel rectangles
    using erf. A four-radius cutoff discards less than 4e-8 of its total energy.
    Layer displacements, opacity, weights and scatter gain remain runtime data.
    """
    edge = DENSITY_EDGE
    texel = WIDTH * PITCH / edge
    result = bytearray()
    integral_factor = math.sqrt(math.pi) * .5 / texel
    for glyph_index in range(COUNT):
        field = np.zeros((edge, edge, 3), dtype=np.float64)
        for layer in range(3):
            for x, y in points[FIRST + glyph_index]:
                cx, cy, radius, glint_seed = fracture_sample(x, y, glyph_index, layer)
                low_x = max(0, int(math.floor((cx - 4 * radius - ORIGIN[0]) / texel)))
                high_x = min(edge, int(math.ceil((cx + 4 * radius - ORIGIN[0]) / texel)))
                low_y = max(0, int(math.floor((cy - 4 * radius - ORIGIN[1]) / texel)))
                high_y = min(edge, int(math.ceil((cy + 4 * radius - ORIGIN[1]) / texel)))
                if high_x <= low_x or high_y <= low_y:
                    continue
                erf_x = np.fromiter((math.erf((ORIGIN[0] + i * texel - cx) / radius)
                                     for i in range(low_x, high_x + 1)), dtype=np.float64)
                erf_y = np.fromiter((math.erf((ORIGIN[1] + i * texel - cy) / radius)
                                     for i in range(low_y, high_y + 1)), dtype=np.float64)
                integrated_x = np.diff(erf_x) * radius * integral_factor
                integrated_y = np.diff(erf_y) * radius * integral_factor
                glint = .55 + 1.8 * glint_seed ** 6
                field[low_y:high_y, low_x:high_x, layer] += glint * np.outer(integrated_y, integrated_x)
        np.clip(field, 0., 1., out=field)
        levels = []
        for level in range(DENSITY_LEVELS):
            # Quantization happens only after each mip was computed in float.
            levels.append(np.rint(field * 255).astype(np.uint16).reshape(-1, 3))
            if level + 1 < DENSITY_LEVELS:
                size = field.shape[0]
                field = field.reshape(size // 2, 2, size // 2, 2, 3).mean(axis=(1, 3))
        rgb = np.concatenate(levels, axis=0)
        assert rgb.shape == (DENSITY_TEXELS_PER_GLYPH - 1, 3)
        packed = np.zeros((DENSITY_TEXELS_PER_GLYPH, 2), dtype="<f4")
        packed[:-1, 0] = rgb[:, 0].astype(np.uint32) + 256 * rgb[:, 1].astype(np.uint32)
        packed[:-1, 1] = rgb[:, 2]
        result.extend(packed.tobytes(order="C"))
    assert len(result) == 16602960
    return bytes(result)


def line_points(line, glyphs, points):
    """Return the same per-layer dot centres consumed by the native shader."""
    font_px = line["font_size_px"]
    pen, baseline = line["baseline_px"]
    result = []
    for char in line["text"]:
        cp = ord(char)
        assert FIRST <= cp < FIRST + COUNT, "Demo supports printable ASCII: " + repr(char)
        for layer, (offset, shift) in enumerate(zip(STYLE["layer_offsets_em"],
                                                   STYLE["layer_shift_cells"])):
            for x, y in points[cp]:
                cx, cy, radius, glint_seed = fracture_sample(x, y, cp - FIRST, layer)
                result.append((pen + (cx + shift[0] * PITCH) * font_px,
                               baseline - (cy + shift[1] * PITCH) * font_px,
                               -offset * font_px, font_px * radius, layer,
                               glint_seed * 2 * math.pi))
        pen += glyphs[cp - FIRST]["advance_em"] * font_px
    return result


def driver(obj, data_path, index, panel, expression):
    fc = obj.driver_add(data_path, index)
    fc.driver.type = "SCRIPTED"
    for name, prop in (("width", "panel_width_m"), ("height", "panel_height_m")):
        variable = fc.driver.variables.new()
        variable.name, variable.type = name, "SINGLE_PROP"
        variable.targets[0].id = panel
        variable.targets[0].data_path = '["' + prop + '"]'
    fc.driver.expression = expression


def fracture_material(layer):
    name = "SSLE / fine internal microfractures / layer " + str(layer)
    mat = bpy.data.materials.get(name)
    if mat is None:
        mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    mat.diffuse_color = tuple(STYLE["base_color_linear_rgb"]) + (1.,)
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    nodes.clear()
    output = nodes.new("ShaderNodeOutputMaterial")
    output.location = (520, 0)
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.location = (-80, 70)
    bsdf.label = "Fine glints from laser-induced internal fractures"
    bsdf.inputs["Base Color"].default_value = tuple(STYLE["base_color_linear_rgb"]) + (1.,)
    bsdf.inputs["Roughness"].default_value = .38
    bsdf.inputs["IOR"].default_value = 1.45
    transparent = nodes.new("ShaderNodeBsdfTransparent")
    transparent.location = (-80, -160)
    mix = nodes.new("ShaderNodeMixShader")
    mix.location = (300, 0)
    mix.inputs[0].default_value = STYLE["opacity"] * STYLE["layer_weights"][layer]
    links.new(transparent.outputs[0], mix.inputs[1])
    links.new(bsdf.outputs[0], mix.inputs[2])
    links.new(mix.outputs[0], output.inputs["Surface"])
    mat["purpose"] = "Embedded rough microfractures, not an emissive surface label"
    mat["layer_weight"] = STYLE["layer_weights"][layer]
    mat["native_scatter_gain"] = STYLE["scatter_gain"]
    mat["parity"] = "Native uses a stylized scattering gain and point brightness; Cycles material has no emission."
    return mat


def build_block(block, glyphs, points, font, collection, materials):
    panel = bpy.data.objects[block["panel_name"]]
    width, height = panel["panel_width_m"], panel["panel_height_m"]
    all_points = [p for line in block["lines"] for p in line_points(line, glyphs, points)]
    assert all_points
    bbox = [min(p[0] - p[3] for p in all_points), min(p[1] - p[3] for p in all_points),
            max(p[0] + p[3] for p in all_points), max(p[1] + p[3] for p in all_points)]
    block["bbox_px"] = bbox
    margin = block["margin_px"]
    padded_w, padded_h = bbox[2] - bbox[0] + 2 * margin, bbox[3] - bbox[1] + 2 * margin
    safe_w, safe_h = width - .31 * min(width, height), height - .31 * min(width, height)
    factor = min(METRES_PER_PX, safe_w / padded_w, safe_h / padded_h)
    block["reference_fit_metres_per_px"] = factor
    group = bpy.data.objects.new("SSLE_" + str(block["panel_index"]) + "_dimension_following", None)
    collection.objects.link(group)
    group.parent = panel
    group["purpose"] = "Uniform glyph scale and internal depth follow the actual panel recut dimensions"
    group["bbox_px"] = bbox
    group["margin_px"] = margin
    scale_expr = f"min({METRES_PER_PX!r},(width-.31*min(width,height))/{padded_w!r},(height-.31*min(width,height))/{padded_h!r})"
    for axis in range(3):
        driver(group, "scale", axis, panel, f"({scale_expr})/{factor!r}")
    driver(group, "location", 0, panel,
           f"-.5*(width-.31*min(width,height))+({margin - bbox[0]!r})*({scale_expr})")
    driver(group, "location", 1, panel,
           f".5*(height-.31*min(width,height))+({bbox[1] - margin!r})*({scale_expr})")
    driver(group, "location", 2, panel, "-.04*min(width,height)")
    for line_index, line in enumerate(block["lines"]):
        name = f"SSLE_SOURCE_{block['panel_index']}_{line_index}"
        curve = font_curve(name, line["text"], font)
        source_obj = bpy.data.objects.new(name, curve)
        collection.objects.link(source_obj)
        source_obj.parent = group
        curve.size = line["font_size_px"] * factor
        curve.offset = -STYLE["stroke_inset_em"] * curve.size
        source_obj.location = (line["baseline_px"][0] * factor, -line["baseline_px"][1] * factor, 0.)
        source_obj["font_size_px"] = line["font_size_px"]
        source_obj["baseline_px"] = line["baseline_px"]
        source_obj["regenerate"] = "Edit body; run the packed make_laser_engraving.py Text block. No automatic scripts."
        source_obj.hide_render = True
        source_obj.hide_set(True)

    # Six vertices/eight facets per dot expose the actual volumetric point style
    # in Blender; native uses filtered density slices at the matching point centres.
    vertices, faces = [], []
    octa = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, .45), (0, 0, -.45))
    topology = ((0, 2, 4), (2, 1, 4), (1, 3, 4), (3, 0, 4),
                (2, 0, 5), (1, 2, 5), (3, 1, 5), (0, 3, 5))
    for x, y, z, radius, _, angle in all_points:
        start = len(vertices)
        cosine, sine = math.cos(angle), math.sin(angle)
        vertices.extend(((x + (dx * cosine - dy * sine) * radius) * factor,
                         (-y + (dx * sine + dy * cosine) * radius) * factor,
                         (z + dz * radius) * factor) for dx, dy, dz in octa)
        faces.extend(tuple(start + i for i in face) for face in topology)
    mesh = bpy.data.meshes.new("SSLE embedded fracture lattice / " + str(block["panel_index"]))
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new("SSLE embedded text / " + str(block["panel_index"]), mesh)
    collection.objects.link(obj)
    obj.parent = group
    for material in materials:
        mesh.materials.append(material)
    for polygon in mesh.polygons:
        polygon.material_index = all_points[polygon.index // len(topology)][4]
    obj["point_count"] = len(all_points)
    obj["text_is_embedded"] = True
    obj["native_glyph_packet"] = "engraving.bin"
    return obj, group, panel


def check_embedding(objects):
    module_spec = importlib.util.spec_from_file_location("ssle_panel_cut", SOURCE / "make_gem_panels.py")
    cut = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(cut)
    report = []
    for mesh, group, panel in objects:
        original = panel["panel_width_m"], panel["panel_height_m"]
        for width, height in (original, (.18, 1.8), (1.8, .18)):
            panel["panel_width_m"], panel["panel_height_m"] = width, height
            panel.update_tag()
            bpy.context.view_layer.update()
            scale = .5 * min(width, height)
            boundaries = cut.planes(width, height)
            transform = group.matrix_local @ mesh.matrix_local
            local_points = [(transform @ vertex.co) / scale + Vector((0, 0, .30))
                            for vertex in mesh.data.vertices]
            worst = max(normal.dot(point) - distance
                        for point in local_points for normal, distance in boundaries)
            assert worst < -1e-5, (panel.name, width, height, "fracture outside gem", worst)
            report.append({"panel": panel.name, "width_m": width, "height_m": height,
                           "max_halfspace_excess_local": worst, "all_fracture_vertices_inside": True})
        panel["panel_width_m"], panel["panel_height_m"] = original
        panel.update_tag()
        bpy.context.view_layer.update()
    return report


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    source_hashes = {name: hashlib.sha256((SOURCE / name).read_bytes()).hexdigest()
                     for name in ("GemPanels.blend", "GemPanels_parametric.blend")}
    output_blend = OUT / "GemPanels_engraved.blend"
    editing_saved_copy = bpy.data.filepath and Path(bpy.data.filepath).resolve() == output_blend.resolve()
    if not editing_saved_copy:
        bpy.ops.wm.open_mainfile(filepath=str(SOURCE / "GemPanels_parametric.blend"))
    else:
        for block in BLOCKS:
            for index, line in enumerate(block["lines"]):
                obj = bpy.data.objects.get(f"SSLE_SOURCE_{block['panel_index']}_{index}")
                if obj and obj.type == "FONT":
                    line["text"] = obj.data.body
                    line["font_size_px"] = float(obj.get("font_size_px", line["font_size_px"]))
                    line["baseline_px"] = list(obj.get("baseline_px", line["baseline_px"]))
    bpy.context.scene["mirror_ssle_repository"] = str(ROOT)
    old = bpy.data.collections.get("SSLE / editable subsurface text")
    if old:
        for obj in list(old.objects):
            bpy.data.objects.remove(obj, do_unlink=True)
        bpy.data.collections.remove(old)
    collection = bpy.data.collections.new("SSLE / editable subsurface text")
    bpy.context.scene.collection.children.link(collection)
    font = bpy.data.fonts.load(str(FONT_PATH), check_existing=True)
    font.pack()
    glyphs, points, packet = make_glyphs(font)
    density_packet = make_density_packet(points)
    materials = [fracture_material(layer) for layer in range(3)]
    objects = [build_block(block, glyphs, points, font, collection, materials) for block in BLOCKS]
    checks = check_embedding(objects)
    contract = {
        "schema": 3, "glyph_first": FIRST, "glyph_count": COUNT,
        "cell_width": WIDTH, "cell_height": HEIGHT, "pitch_em": PITCH, "origin_em": list(ORIGIN),
        "rows_per_glyph": 144, "words_per_glyph": 576, "bits_per_word": 16,
        "binary": "engraving.bin", "binary_sha256": hashlib.sha256(packet).hexdigest(),
        "density_binary": "density.bin", "density_sha256": hashlib.sha256(density_packet).hexdigest(),
        "density_edge": DENSITY_EDGE, "density_levels": DENSITY_LEVELS,
        "density_texels_per_glyph": DENSITY_TEXELS_PER_GLYPH,
        "density_packing": "Per glyph: mip edges 128,64,32,16,8,4,2,1 in y-ascending row order, then one zero texel. Three layer channels are quantized to 8 bits after float mip generation. Two little-endian float32 values per texel: R+256*G, B; two texels per RGBA row.",
        "density_domain": "Same origin_em and 1.2 em extent as occupancy. Each point contributes glint*exp(-distance_squared/radius_squared), analytically integrated over texel area with separable erf. Clamp base density to [0,1], then average float mips. Layer shifts, weights, opacity and scatter gain are not baked.",
        "packing": "little-endian float32 RGBA rows; each float is one exact 16-bit occupancy integer; y ascending then x",
        "font": FONT_PATH.name, "font_sha256": hashlib.sha256(FONT_PATH.read_bytes()).hexdigest(),
        "glyph_extraction": "Blender filled FONT evaluated mesh triangles with .008 em inward stroke offset; sample each lattice cell centre. Original font metrics are measured without the offset.",
        "advance_extraction": "width(A+glyph+A)-width(AA) in the same Blender font; no string kerning in native layout",
        "glyphs": glyphs, "style": STYLE, "demo_blocks": BLOCKS,
        "point_hash": "n=(x*73+y*151+glyphIndex*199+layer*283+salt*317)%997; h=((n*n*157+n*313)%1009)/1009; glyphIndex=codepoint-32",
        "point_variation": "salts 1/2: XY jitter; salt 3: radius; salt 4: octahedron Z rotation; all occupied cells retained",
        "native_style": "Baked narrow microfracture-density fields accelerate the same three interior depth slices with faint shifted wisps; this is not a screen overlay. Point brightness .55+1.8*hash(salt4)^6 is baked; native applies style.scatter_gain. Cycles material is non-emissive. Exact Cycles appearance parity is not claimed.",
        "retired_style": "Schema 1 equal-strength depth slices and broad reconstruction merged individual fracture points into heavy blurred strokes.",
        "reference_viewport_px": [1280, 720], "reference_metres_per_px": METRES_PER_PX,
        "fit": "uniform min(reference_metres_per_px, safe_width/(bbox_width+2*margin), safe_height/(bbox_height+2*margin)); safe dimensions = panel dimensions - .31*minor; top-left align padded bbox",
        "source_blend_sha256": source_hashes,
        "blender_version": bpy.app.version_string,
        "scope": "Decorative ASCII example and runtime glyph data, no browser/settings functionality. Blender uses faceted microfractures; native uses filtered density slices at matching fracture centres.",
    }
    (OUT / "engraving.bin").write_bytes(packet)
    (OUT / "density.bin").write_bytes(density_packet)
    (OUT / "engraving.json").write_text(json.dumps(contract, indent=2) + "\n")
    (OUT / "engraving_checks.json").write_text(json.dumps({"glyph_count": len(glyphs),
        "binary_bytes": len(packet), "density_bytes": len(density_packet),
        "A_occupied_cells": len(points[ord("A")]),
        "space_occupied_cells": len(points[32]), "embedding_checks": checks}, indent=2) + "\n")
    packed_script = bpy.data.texts.get("make_laser_engraving.py") or bpy.data.texts.new("make_laser_engraving.py")
    packed_script.clear()
    packed_script.write((ROOT / "BuildSupport/Blender/make_laser_engraving.py").read_text())
    bpy.context.scene["ssle_instructions"] = "Edit hidden SSLE_SOURCE_* FONT bodies, then Run Script on make_laser_engraving.py. Packed font and actual microfracture meshes; no auto-run."
    license_text = bpy.data.texts.get("AlegreyaSans-OFL.txt") or bpy.data.texts.new("AlegreyaSans-OFL.txt")
    license_text.clear()
    license_text.write((ROOT / "BuildSupport/Licenses/AlegreyaSans-OFL.txt").read_text())
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(output_blend))
    assert all(hashlib.sha256((SOURCE / name).read_bytes()).hexdigest() == digest
               for name, digest in source_hashes.items()), "Original panel files changed"
    print("SSLE_AUTHOR_OK=" + json.dumps({"blend": str(output_blend), "glyphs": len(glyphs),
        "packet_bytes": len(packet), "embedding_checks": len(checks),
        "density_bytes": len(density_packet),
        "fracture_points": sum(obj[0]["point_count"] for obj in objects)}))


if __name__ == "__main__":
    main()
