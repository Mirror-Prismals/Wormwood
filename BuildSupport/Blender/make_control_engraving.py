"""Author internal transport/fader engraving without modifying the original kits.

Blender --background --factory-startup --python-exit-code 1 --python \
    BuildSupport/Blender/make_control_engraving.py

Actual faceted fracture geometry lives in the editable .blend copies. Native GLBs
contain the original crystal/control meshes, with only the replaced opaque marks
removed; matching density data supplies the interior engraving in the renderer.
No render, application build, or runtime shader modification is performed here.
"""
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import struct

import bpy
import numpy as np


ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "Art/AudioWorkstation"
OUT = ART / "ControlEngraving"


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


ssle = module("control_ssle_style", ROOT / "BuildSupport/Blender/make_laser_engraving.py")
gem = module("control_ssle_gem", ART / "GemPanels/make_gem_panels.py")
STYLE = dict(ssle.STYLE)
UNIT_M = .020
PITCH_M = ssle.PITCH * UNIT_M
SYMBOLS = ("Rewind", "Stop", "Record", "Play")
OCTA = ((1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0),
        (0, 0, .45), (0, 0, -.45))
FACES = ((0, 2, 4), (2, 1, 4), (1, 3, 4), (3, 0, 4),
         (2, 0, 5), (1, 2, 5), (3, 1, 5), (0, 3, 5))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def mesh_signature(obj):
    """Catch accidental host geometry/material/parent-local transform changes."""
    value = {
        "vertices": [list(v.co) for v in obj.data.vertices],
        "faces": [list(p.vertices) for p in obj.data.polygons],
        "material_indices": [p.material_index for p in obj.data.polygons],
        "materials": [m.name for m in obj.data.materials],
        "matrix_local": [list(row) for row in obj.matrix_local],
    }
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def projected_triangles(objects, host):
    """Sample the existing opaque marks' actual XY silhouettes, not new icons."""
    result = []
    inverse = host.matrix_world.inverted()
    for obj in objects:
        transform = inverse @ obj.matrix_world
        obj.data.calc_loop_triangles()
        for triangle in obj.data.loop_triangles:
            vertices = [transform @ obj.data.vertices[i].co for i in triangle.vertices]
            a, b, c = vertices
            twice_area = (b.x-a.x)*(c.y-a.y)-(b.y-a.y)*(c.x-a.x)
            if abs(twice_area) > 1e-14:
                result.append(tuple((v.x, v.y) for v in vertices))
    assert result, "Opaque source has no filled face silhouette"
    return result


def make_pattern(pattern_id, index, source_objects, host, track=False):
    triangles = projected_triangles(source_objects, host)
    origin = [-.012, -.085] if track else [-.012, -.012]
    extent = [.024, .192] if track else [.024, .024]
    cells = [96, 768] if track else [96, 96]
    pixels = [128, 1024] if track else [128, 128]
    occupied = []
    for y in range(cells[1]):
        for x in range(cells[0]):
            position = (origin[0]+(x+.5)*PITCH_M, origin[1]+(y+.5)*PITCH_M)
            if any(ssle.contains(position, tri) for tri in triangles):
                occupied.append((x, y))
    assert occupied
    assert all(origin[axis] < v[axis] < origin[axis]+extent[axis]
               for tri in triangles for v in tri for axis in range(2))
    return {
        "id": pattern_id, "hash_index": index, "origin_m": origin,
        "extent_m": extent, "unit_m": UNIT_M, "pitch_m": PITCH_M,
        "cell_size": cells, "density_base_size_px": pixels,
        "occupied_cells": len(occupied), "_occupied": occupied,
        "source_outline": "Union of projected triangles from the original opaque source meshes",
    }


def sample(pattern, x, y, layer):
    def noise(salt):
        return ssle.fracture_hash(x, y, pattern["hash_index"], layer, salt)
    px = pattern["origin_m"][0]+(x+.5)*PITCH_M
    py = pattern["origin_m"][1]+(y+.5)*PITCH_M
    jitter = STYLE["point_jitter_cells"] * 2 * PITCH_M
    px += (noise(1)-.5)*jitter
    py += (noise(2)-.5)*jitter
    low, high = STYLE["point_radius_range"]
    radius = UNIT_M*STYLE["radius_em"]*(low+(high-low)*noise(3))
    return px, py, radius, noise(4)


def bake_density(pattern):
    """Area-integrated RGB density, with rectangular float mips before packing."""
    width, height = pattern["density_base_size_px"]
    origin, extent = pattern["origin_m"], pattern["extent_m"]
    texel_x, texel_y = extent[0]/width, extent[1]/height
    factor_x, factor_y = math.sqrt(math.pi)*.5/texel_x, math.sqrt(math.pi)*.5/texel_y
    field = np.zeros((height, width, 3), dtype=np.float64)
    for layer in range(3):
        for x, y in pattern["_occupied"]:
            cx, cy, radius, glint_seed = sample(pattern, x, y, layer)
            x0 = max(0, int(math.floor((cx-4*radius-origin[0])/texel_x)))
            x1 = min(width, int(math.ceil((cx+4*radius-origin[0])/texel_x)))
            y0 = max(0, int(math.floor((cy-4*radius-origin[1])/texel_y)))
            y1 = min(height, int(math.ceil((cy+4*radius-origin[1])/texel_y)))
            if x1 <= x0 or y1 <= y0:
                continue
            ex = np.fromiter((math.erf((origin[0]+i*texel_x-cx)/radius)
                              for i in range(x0, x1+1)), dtype=np.float64)
            ey = np.fromiter((math.erf((origin[1]+i*texel_y-cy)/radius)
                              for i in range(y0, y1+1)), dtype=np.float64)
            ix, iy = np.diff(ex)*radius*factor_x, np.diff(ey)*radius*factor_y
            field[y0:y1, x0:x1, layer] += (.55+1.8*glint_seed**6)*np.outer(iy, ix)
    np.clip(field, 0., 1., out=field)
    levels, descriptors, offset = [], [], 0
    while True:
        height, width = field.shape[:2]
        descriptors.append({"size_px": [width, height], "offset_texels": offset})
        levels.append(np.rint(field*255).astype(np.uint16).reshape(-1, 3))
        offset += width*height
        if width == 1 and height == 1:
            break
        next_width, next_height = max(1, width//2), max(1, height//2)
        field = field.reshape(next_height, height//next_height,
                              next_width, width//next_width, 3).mean(axis=(1, 3))
    rgb = np.concatenate(levels, axis=0)
    padded = offset + (offset % 2)
    packed = np.zeros((padded, 2), dtype="<f4")
    packed[:offset, 0] = rgb[:, 0].astype(np.uint32)+256*rgb[:, 1].astype(np.uint32)
    packed[:offset, 1] = rgb[:, 2]
    pattern["density_mips"] = descriptors
    pattern["density_texels_padded"] = padded
    pattern["density_rows"] = padded//2
    return packed.tobytes(order="C")


def fracture_geometry(pattern, host, collection, layer_z):
    vertices, faces, layers = [], [], []
    for layer in range(3):
        sx, sy = STYLE["layer_shift_cells"][layer]
        for x, y in pattern["_occupied"]:
            cx, cy, radius, glint = sample(pattern, x, y, layer)
            cx, cy = cx+sx*PITCH_M, cy+sy*PITCH_M
            cosine, sine = math.cos(glint*math.tau), math.sin(glint*math.tau)
            start = len(vertices)
            vertices.extend((cx+(dx*cosine-dy*sine)*radius,
                             cy+(dx*sine+dy*cosine)*radius,
                             layer_z[layer]+dz*radius) for dx, dy, dz in OCTA)
            faces.extend(tuple(start+i for i in face) for face in FACES)
            layers.extend([layer]*len(FACES))
    mesh = bpy.data.meshes.new(host.name+"_SSLE_Microfractures")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(host.name+"_SSLE_Display", mesh)
    collection.objects.link(obj)
    obj.parent = host
    for layer in range(3):
        mesh.materials.append(ssle.fracture_material(layer))
    mesh.polygons.foreach_set("material_index", layers)
    obj["role"] = "SSLE_AUTHORING_DISPLAY_ONLY"
    obj["ssle_pattern"] = pattern["id"]
    obj["native_representation"] = "Interior density slices in control_engraving.bin; omit this display mesh from runtime triangle packets"
    obj["point_count"] = len(pattern["_occupied"])*3
    obj["all_points_retained"] = True
    return obj


def embedding_check(display, host, surface_z):
    """Check every actual octahedron vertex, including the fader's recess floor."""
    width, height = float(host["nominal_width_m"]), float(host["nominal_height_m"])
    scale = .5*min(width, height)
    planes = gem.planes(width, height)
    normals = np.array([list(n) for n, _ in planes], dtype=np.float64)
    distances = np.array([scale*(d-.30*n.z) for n, d in planes], dtype=np.float64)
    lengths = np.linalg.norm(normals, axis=1)
    normals, distances = normals/lengths[:, None], distances/lengths
    coords = np.empty(len(display.data.vertices)*3, dtype=np.float32)
    display.data.vertices.foreach_get("co", coords)
    coords = coords.reshape(-1, 3)
    worst = -float("inf")
    for start in range(0, len(coords), 32768):
        worst = max(worst, float(np.max(coords[start:start+32768] @ normals.T-distances)))
    highest = float(np.max(coords[:, 2]))
    assert worst < -1e-7, (host.name, "Fracture leaves original gem cut", worst)
    assert highest < surface_z-1e-7, (host.name, "Fracture leaves host solid at table/recess", highest)
    return {"host": host.name, "vertices_checked": len(coords),
            "max_outer_halfspace_excess_m": worst,
            "highest_fracture_z_m": highest, "local_surface_z_m": surface_z,
            "all_vertices_embedded": True}


def export_native(root, original_glb):
    """Export unchanged solid parts only; the .blend keeps all fracture geometry."""
    bpy.ops.object.select_all(action="DESELECT")
    selected = [root] + list(root.children_recursive)
    for obj in selected:
        if obj.get("role") != "SSLE_AUTHORING_DISPLAY_ONLY":
            obj.select_set(True)
    bpy.context.view_layer.objects.active = root
    old_location = root.location.copy()
    try:
        root.location = (0, 0, 0)
        bpy.context.view_layer.update()
        path = OUT / (root.name+".glb")
        bpy.ops.export_scene.gltf(filepath=str(path), export_format="GLB", use_selection=True,
                                  export_apply=True, export_extras=True, export_animations=False, export_cameras=False,
                                  export_lights=False, export_yup=True)
    finally:
        root.location = old_location
        bpy.context.view_layer.update()
    # Preserve the original asset's authored volume/material extensions exactly.
    source = original_glb.read_bytes()
    source_length, source_tag = struct.unpack_from("<II", source, 12)
    assert source_tag == 0x4e4f534a
    source_doc = json.loads(source[20:20+source_length])
    source_materials = {m["name"]: m for m in source_doc["materials"]}
    raw = path.read_bytes()
    length, tag = struct.unpack_from("<II", raw, 12)
    assert tag == 0x4e4f534a
    document = json.loads(raw[20:20+length])
    for material in document["materials"]:
        assert material["name"] in source_materials, material["name"]
        source_mat = source_materials[material["name"]]
        # These original material dictionaries carry no image/texture indices.
        assert not any("Texture" in key for key in source_mat.get("pbrMetallicRoughness", {}))
        material.clear()
        material.update(source_mat)
    extensions = set(document.get("extensionsUsed", []))
    extensions.update(key for m in document["materials"] for key in m.get("extensions", {}))
    document["extensionsUsed"] = sorted(extensions)
    assert not any(n.get("extras", {}).get("role") == "SSLE_AUTHORING_DISPLAY_ONLY"
                   for n in document["nodes"])
    assert all(p.get("mode", 4) == 4 for m in document["meshes"] for p in m["primitives"])
    payload = json.dumps(document, separators=(",", ":")).encode()
    payload += b" "*(-len(payload) % 4)
    rest = raw[20+length:]
    path.write_bytes(struct.pack("<III", 0x46546c67, 2, 20+len(payload)+len(rest))
                     + struct.pack("<II", len(payload), 0x4e4f534a)+payload+rest)
    return path, document


def author_asset(name, source_rel, pattern_id, index, patterns, binary, checks, track=False):
    root = bpy.data.objects[name]
    host = bpy.data.objects[name+("_Housing" if track else "_Cap")]
    source_nodes = ([bpy.data.objects[name+"_Housing_WellLiner"]] if track else
                    [obj for obj in root.children_recursive if obj.name.startswith(name+"_Symbol_")])
    assert source_nodes
    excluded = {obj.name for obj in source_nodes}
    preserved = {obj.name: mesh_signature(obj) for obj in root.children_recursive
                 if obj.type == "MESH" and obj.name not in excluded}
    # All target hosts currently share the root coordinate frame. Fail loudly if
    # a future authoring change introduces a host transform needing native support.
    assert all(abs(host.matrix_local[i][j]-float(i == j)) < 1e-8
               for i in range(4) for j in range(4)), host.name
    candidate = make_pattern(pattern_id, index, source_nodes, host, track)
    existing = next((p for p in patterns if p["id"] == pattern_id), None)
    if existing:
        assert candidate["_occupied"] == existing["_occupied"], "Variant track silhouettes diverged"
        pattern = existing
    else:
        pattern = candidate
        pattern["density_offset_rows"] = len(binary)//16
        binary.extend(bake_density(pattern))
        patterns.append(pattern)
    surface_z = -.0018 if track else 0.
    front_z = surface_z-STYLE["front_depth_minor_fraction"]*min(
        float(host["nominal_width_m"]), float(host["nominal_height_m"]))
    layer_z = [front_z-offset*UNIT_M for offset in STYLE["layer_offsets_em"]]
    display = fracture_geometry(pattern, host, root.users_collection[0], layer_z)
    checks.append(embedding_check(display, host, surface_z))
    removed_names = [obj.name for obj in source_nodes]
    for obj in source_nodes:
        bpy.data.objects.remove(obj, do_unlink=True)
    host["ssle_pattern"] = pattern_id
    host["ssle_layer_z_m"] = layer_z
    host["ssle_native_manifest"] = "control_engraving.json"
    host["ssle_scope"] = "Internal decorative microfractures; original host geometry and material preserved"
    root["ssle_replaced_nodes"] = json.dumps(removed_names)
    path, document = export_native(root, ART/source_rel)
    for obj_name, signature in preserved.items():
        assert mesh_signature(bpy.data.objects[obj_name]) == signature, (obj_name, "Original component changed")
    host_material = host.data.materials[0].name
    assert any(n.get("name") == host.name for n in document["nodes"])
    assert not any(n.get("name") in removed_names for n in document["nodes"])
    return {
        "name": name, "glb": path.name, "glb_sha256": digest(path),
        "source_glb": source_rel, "source_glb_sha256": digest(ART/source_rel),
        "host_node": host.name, "host_material": host_material,
        "host_material_index": next(i for i, m in enumerate(document["materials"])
                                    if m["name"] == host_material),
        "pattern_id": pattern_id, "layer_z_m": layer_z,
        "source_surface_z_m": surface_z, "removed_nodes": removed_names,
        "unchanged_components": sorted(preserved), "fracture_points": display["point_count"],
        "native_glb_has_fracture_triangles": False,
        "geometry_policy": "Original host solid, facets, recess, and remaining control parts preserved",
    }


def save_copy(filename):
    text = bpy.data.texts.get("make_control_engraving.py") or bpy.data.texts.new("make_control_engraving.py")
    text.clear()
    text.write((ROOT/"BuildSupport/Blender/make_control_engraving.py").read_text())
    bpy.context.scene["ssle_repository"] = str(ROOT)
    bpy.context.scene["ssle_instructions"] = "Actual editable microfracture meshes. Rebuild from original kits with BuildSupport/Blender/make_control_engraving.py; no automatic scripts. Native GLBs use the matching interior density packet."
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/filename))


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    original_paths = [ART/"GemButtons/GemButtons.blend", ART/"GemFadersMeters/GemFadersMeters.blend"]
    original_paths += [ART/("GemButtons/GemButton_"+name+".glb") for name in SYMBOLS]
    original_paths += [ART/("GemFadersMeters/GemFader_"+name+".glb") for name in ("Ruby", "Ivory")]
    before = {str(path.relative_to(ART)): digest(path) for path in original_paths}
    patterns, assets, checks, binary = [], [], [], bytearray()
    bpy.ops.wm.open_mainfile(filepath=str(ART/"GemButtons/GemButtons.blend"))
    bpy.context.view_layer.update()
    for index, suffix in enumerate(SYMBOLS):
        name = "GemButton_"+suffix
        assets.append(author_asset(name, "GemButtons/"+name+".glb", "transport_"+suffix.lower(),
                                   index, patterns, binary, checks))
    save_copy("GemButtons_engraved.blend")
    bpy.ops.wm.open_mainfile(filepath=str(ART/"GemFadersMeters/GemFadersMeters.blend"))
    bpy.context.view_layer.update()
    for suffix in ("Ruby", "Ivory"):
        name = "GemFader_"+suffix
        assets.append(author_asset(name, "GemFadersMeters/"+name+".glb", "fader_track", 4,
                                   patterns, binary, checks, track=True))
    save_copy("GemFadersMeters_engraved.blend")
    manifest = {
        "schema": 1, "density_binary": "control_engraving.bin",
        "density_sha256": hashlib.sha256(binary).hexdigest(), "density_bytes": len(binary),
        "density_rows": len(binary)//16, "row_bytes": 16,
        "density_packing": "Little-endian float32 pairs per texel: R+256*G, B, with 8-bit density channels. Two texels per RGBA row. Per-pattern mips run largest to 1x1, y ascending, x fastest; one zero texel pads each odd-length pattern to a whole row.",
        "density_baking": "Three unshifted layer fields. Each point contributes (.55+1.8*hash4^6)*exp(-distance_squared/radius_squared), analytically integrated over texel area. Clamp base to [0,1], average float mips, then quantize. Layer shifts, weights, opacity and scatter gain are not baked.",
        "coordinate_space": "Unchanged host/root-local Blender metres; +X right, +Y along face, +Z outward. All target hosts have identity transforms relative to their asset root.",
        "style": STYLE,
        "point_hash": "n=(x*73+y*151+hash_index*199+layer*283+salt*317)%997; h=((n*n*157+n*313)%1009)/1009",
        "layer_shift_m": [[sx*PITCH_M, sy*PITCH_M] for sx, sy in STYLE["layer_shift_cells"]],
        "patterns": [{k: v for k, v in p.items() if not k.startswith("_")} for p in patterns],
        "assets": assets,
        "native_header_row": 244,
        "native_descriptor": "Host material row+2.w points to rows [origin.x,origin.y,extent.x,extent.y], [layerZ0,layerZ1,layerZ2,unit_m], [patternDensityOffsetRows,mipCount,absoluteMipTableRow,0], followed by [width,height,offsetTexels,0] for each mip. Header244=[immutableDensityBase,density_rows,0,0]. Existing panel style globals are shared.",
        "blend_files": ["GemButtons_engraved.blend", "GemFadersMeters_engraved.blend"],
        "source_sha256": before,
        "scope": "Decorative transport symbols and fader tracks only. Physical recess retained; slider, index, stem, meter assets, outer crystal shape and material unchanged. Actual fracture geometry in Blender; native GLBs omit fracture-display meshes and use matching internal density slices.",
        "blender_version": bpy.app.version_string,
    }
    assert len(binary) % 16 == 0
    assert len(patterns) == 5 and len(assets) == 6
    assert len(binary) == 2097216, len(binary)
    (OUT/"control_engraving.bin").write_bytes(binary)
    (OUT/"control_engraving.json").write_text(json.dumps(manifest, indent=2)+"\n")
    (OUT/"control_engraving_checks.json").write_text(json.dumps({
        "embedding_checks": checks, "source_kits_unchanged": True,
        "unchanged_component_checks": sum(len(a["unchanged_components"]) for a in assets),
        "patterns": len(patterns), "assets": len(assets), "density_rows": len(binary)//16,
    }, indent=2)+"\n")
    assert all(digest(ART/name) == value for name, value in before.items()), "Original kit file changed"
    print("CONTROL_SSLE_AUTHOR_OK="+json.dumps({"output": str(OUT), "patterns": len(patterns),
        "assets": len(assets), "density_bytes": len(binary), "density_rows": len(binary)//16,
        "fracture_points": sum(a["fracture_points"] for a in assets)}))


if __name__ == "__main__":
    main()
