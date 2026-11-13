"""Preserve authored gem art and make a self-contained parametric Blender copy.

Run Blender --background Art/AudioWorkstation/GemPanels/GemPanels.blend
    --python BuildSupport/Blender/prepare_gem_panels.py

The geometry modifier reconstructs vertices from the three intersecting source
planes. Width/height edits therefore recut the crown and pavilion rather than
stretching a finished gem. The saved modifier works without Python auto-run.
The same operation exports the actual material, lights and OCIO display LUT.
No render is started, and the original .blend and GLBs are left untouched.
"""
import array
import hashlib
import importlib.util
import itertools
import json
import math
from pathlib import Path

import bpy
import PyOpenColorIO as ocio
import numpy as np
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "Art/AudioWorkstation/GemPanels"
spec = importlib.util.spec_from_file_location("gem_panel_source", ASSETS / "make_gem_panels.py")
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class Graph:
    def __init__(self, name):
        self.tree = bpy.data.node_groups.new(name, "GeometryNodeTree")
        self.input = self.tree.nodes.new("NodeGroupInput")
        self.output = self.tree.nodes.new("NodeGroupOutput")
        self.count = 0

    def socket(self, name, kind, direction, default=None):
        value = self.tree.interface.new_socket(name=name, in_out=direction, socket_type=kind)
        if default is not None:
            value.default_value = default
        return value

    def node(self, kind):
        node = self.tree.nodes.new(kind)
        self.count += 1
        node.location = ((self.count % 8) * 190, -(self.count // 8) * 150)
        return node

    def set(self, target, value):
        if isinstance(value, bpy.types.NodeSocket):
            self.tree.links.new(value, target)
        else:
            target.default_value = value

    def math(self, op, a, b=None):
        node = self.node("ShaderNodeMath")
        node.operation = op
        self.set(node.inputs[0], a)
        if b is not None:
            self.set(node.inputs[1], b)
        return node.outputs[0]

    def vector(self, op, a, b=None, scale=None):
        node = self.node("ShaderNodeVectorMath")
        node.operation = op
        self.set(node.inputs[0], a)
        if b is not None:
            self.set(node.inputs[1], b)
        if scale is not None:
            self.set(node.inputs[3], scale)
        return node.outputs[1 if op in ("DOT_PRODUCT", "LENGTH", "DISTANCE") else 0]

    def combine(self, x, y, z):
        node = self.node("ShaderNodeCombineXYZ")
        for socket, value in zip(node.inputs, (x, y, z)):
            self.set(socket, value)
        return node.outputs[0]

    def choose(self, condition, false_value, true_value, vector=False):
        node = self.node("GeometryNodeSwitch")
        node.input_type = "VECTOR" if vector else "FLOAT"
        self.set(node.inputs["Switch"], condition)
        self.set(node.inputs["False"], false_value)
        self.set(node.inputs["True"], true_value)
        return node.outputs[0]


def plane_group():
    g = Graph("Mirror Gem / source halfspace")
    for name, default in (("Plane", 0), ("Width", .96), ("Height", .45),
                          ("Depth", .85), ("Crown", .31)):
        g.socket(name, "NodeSocketFloat", "INPUT", default)
    g.socket("Normal", "NodeSocketVector", "OUTPUT")
    g.socket("Distance", "NodeSocketFloat", "OUTPUT")
    index, width, height, depth, crown = [g.input.outputs[name] for name in
                                         ("Plane", "Width", "Height", "Depth", "Crown")]
    minimum = g.math("MINIMUM", width, height)
    a, b = g.math("DIVIDE", width, minimum), g.math("DIVIDE", height, minimum)
    n = g.math("SUBTRACT", index, 2)
    ring = g.math("FLOOR", g.math("DIVIDE", n, 8))
    side = g.math("MODULO", n, 8)
    pavilion = g.math("GREATER_THAN", ring, 1.5)
    is_crown = g.math("MULTIPLY", g.math("GREATER_THAN", ring, .5), g.math("LESS_THAN", ring, 1.5))
    angle = g.math("ADD", g.math("MULTIPLY", side, math.pi / 4),
                   g.math("MULTIPLY", pavilion, math.pi / 8))
    x, y = g.math("COSINE", angle), g.math("SINE", angle)
    ax, ay = g.math("ABSOLUTE", x), g.math("ABSOLUTE", y)
    support = g.math("SUBTRACT", g.math("ADD", g.math("MULTIPLY", ax, a),
                                       g.math("MULTIPLY", ay, b)),
                     g.math("MULTIPLY", .18, g.math("MINIMUM", ax, ay)))
    slope = g.math("DIVIDE", g.math("MULTIPLY", support, .86), g.math("MAXIMUM", g.math("SUBTRACT", depth, .07), .1))
    z = g.choose(is_crown, 0., g.math("DIVIDE", crown, .30))
    z = g.choose(pavilion, z, g.math("MULTIPLY", slope, -1))
    normal = g.combine(x, y, z)
    distance = g.choose(pavilion, support, g.math("ADD", support, g.math("MULTIPLY", slope, .07)))
    is_culet = g.math("LESS_THAN", index, 1.5)
    normal = g.choose(is_culet, normal, (0, 0, -1), vector=True)
    distance = g.choose(is_culet, distance, depth)
    is_table = g.math("LESS_THAN", index, .5)
    normal = g.choose(is_table, normal, (0, 0, 1), vector=True)
    distance = g.choose(is_table, distance, .30)
    g.set(g.output.inputs["Normal"], normal)
    g.set(g.output.inputs["Distance"], distance)
    return g.tree


def recut_group(plane_tree):
    g = Graph("Mirror Gem / dimensional recut")
    g.tree.is_modifier = True
    g.socket("Geometry", "NodeSocketGeometry", "INPUT")
    g.socket("Geometry", "NodeSocketGeometry", "OUTPUT")
    for name, default in (("Width", .96), ("Height", .45)):
        item = g.socket(name, "NodeSocketFloat", "INPUT", default)
        item.min_value = .001 if name in ("Width", "Height") else .2 if name == "Depth" else .01
        item.max_value = 100 if name in ("Width", "Height") else 2 if name == "Depth" else .4
        item.description = "Re-intersect Cardinal's source planes; do not scale the finished cut"
    planes = []
    for attr in ("source_plane_a", "source_plane_b", "source_plane_c"):
        named = g.node("GeometryNodeInputNamedAttribute")
        named.data_type = "INT"
        named.inputs["Name"].default_value = attr
        node = g.node("GeometryNodeGroup")
        node.node_tree = plane_tree
        g.set(node.inputs["Plane"], named.outputs["Attribute"])
        for name in ("Width", "Height"):
            g.set(node.inputs[name], g.input.outputs[name])
        node.inputs["Depth"].default_value = .85
        node.inputs["Crown"].default_value = .31
        planes.append((node.outputs["Normal"], node.outputs["Distance"]))
    (a, da), (b, db), (c, dc) = planes
    bc, ca, ab = [g.vector("CROSS_PRODUCT", p, q) for p, q in ((b, c), (c, a), (a, b))]
    determinant = g.vector("DOT_PRODUCT", a, bc)
    terms = [g.vector("SCALE", v, scale=d) for v, d in ((bc, da), (ca, db), (ab, dc))]
    position = g.vector("ADD", g.vector("ADD", terms[0], terms[1]), terms[2])
    position = g.vector("SCALE", position, scale=g.math("DIVIDE", 1., determinant))
    position = g.vector("ADD", position, (0, 0, -.30))
    scale = g.math("MULTIPLY", .5, g.math("MINIMUM", g.input.outputs["Width"], g.input.outputs["Height"]))
    position = g.vector("SCALE", position, scale=scale)
    set_position = g.node("GeometryNodeSetPosition")
    g.set(set_position.inputs["Geometry"], g.input.outputs["Geometry"])
    g.set(set_position.inputs["Position"], position)
    g.set(g.output.inputs["Geometry"], set_position.outputs["Geometry"])
    return g.tree


def driver(socket, obj, expression):
    fc = socket.driver_add("default_value")
    fc.driver.type = "SCRIPTED"
    for name, prop in (("width", "panel_width_m"), ("height", "panel_height_m")):
        var = fc.driver.variables.new()
        var.name, var.type = name, "SINGLE_PROP"
        var.targets[0].id = obj
        var.targets[0].data_path = '["' + prop + '"]'
    fc.driver.expression = expression


def parametrize(obj, tree):
    width, height = obj["nominal_width_m"], obj["nominal_height_m"]
    boundaries = source.planes(width, height)
    scale = min(width, height) * .5
    attrs = [obj.data.attributes.new(name="source_plane_" + key, type="INT", domain="POINT")
             for key in ("a", "b", "c")]
    for vertex in obj.data.vertices:
        point = vertex.co / scale + Vector((0, 0, .30))
        active = [i for i, (n, d) in enumerate(boundaries) if abs(n.dot(point) - d) < 1e-5]
        triples = list(itertools.combinations(active, 3))
        ids = max(triples, key=lambda t: abs(boundaries[t[0]][0].dot(boundaries[t[1]][0].cross(boundaries[t[2]][0]))))
        for attr, plane_id in zip(attrs, ids):
            attr.data[vertex.index].value = plane_id
    modifier = obj.modifiers.new("Parametric Cardinal cut", "NODES")
    modifier.node_group = tree
    for name, prop, value, low, high in (("Width", "panel_width_m", width, .001, 100),
                                       ("Height", "panel_height_m", height, .001, 100)):
        obj[prop] = value
        obj.id_properties_ui(prop).update(min=low, max=high, description="Animatable source-plane recut parameter")
        socket = next(s for s in tree.interface.items_tree if s.name == name and s.in_out == "INPUT")
        modifier[socket.identifier] = value
        fc = modifier.driver_add('["' + socket.identifier + '"]')
        fc.driver.type = "SCRIPTED"
        variable = fc.driver.variables.new()
        variable.name, variable.type = "parameter", "SINGLE_PROP"
        variable.targets[0].id = obj
        variable.targets[0].data_path = '["' + prop + '"]'
        fc.driver.expression = "parameter"
    obj["dimension_editing"] = "Animate panel_width_m and panel_height_m; the fixed Cardinal cut is reconstructed by a self-contained modifier."
    # Equal optical depth across dimension edits is how the original cut works.
    volume = next(n for n in obj.data.materials[0].node_tree.nodes if n.type == "VOLUME_ABSORPTION")
    sigma = max(obj.data.materials[0]["source_sigma_per_local_unit"])
    driver(volume.inputs["Density"], obj, repr(sigma * 2) + "/min(width,height)")


def nodes(material):
    def value(socket):
        v = socket.default_value
        return list(v) if hasattr(v, "__iter__") else v
    return {"nodes": [{"name": n.name, "type": n.bl_idname,
                        "inputs": {s.name: value(s) for s in n.inputs if hasattr(s, "default_value")}}
                       for n in material.node_tree.nodes],
            "links": [[l.from_node.name, l.from_socket.name, l.to_node.name, l.to_socket.name]
                      for l in material.node_tree.links]}


def export_lut(scene):
    config_path = Path(bpy.utils.resource_path("LOCAL")) / "datafiles/colormanagement/config.ocio"
    config = ocio.Config.CreateFromFile(str(config_path))
    transform = ocio.DisplayViewTransform(src=config.getRoleColorSpace("scene_linear"),
                                         display=scene.display_settings.display_device,
                                         view=scene.view_settings.view_transform)
    processor = config.getProcessor(transform)
    cpu = processor.getDefaultCPUProcessor()
    gpu = ocio.GpuShaderDesc.CreateShaderDesc()
    gpu.setLanguage(ocio.GPU_LANGUAGE_GLSL_4_0)
    gpu.setFunctionName("mirror_agx_reference")
    processor.getDefaultGPUProcessor().extractGpuShaderInfo(gpu)
    textures = list(gpu.get3DTextures())
    assert len(textures) == 1 and not list(gpu.getTextures()), "Blender OCIO transform changed; inspect before export"
    texture = textures[0]
    edge = texture.edgeLen
    assert edge == 57, "Blender OCIO transform changed; inspect before export"
    # OCIO provides blue-fastest values. Runtime storage uses red fastest.
    native = np.asarray(texture.getValues(), dtype=np.float32).reshape(edge, edge, edge, 3)
    values = np.ones((edge, edge, edge, 4), dtype="<f4")
    values[:, :, :, :3] = native.transpose(2, 1, 0, 3)
    path = ASSETS / "GemPanels_AgX.bin"
    path.write_bytes(values.tobytes())
    reference_path = ASSETS / "GemPanels_AgX_reference.glsl"
    reference_path.write_text(gpu.getShaderText())
    # These are the operations emitted by the installed Blender OCIO processor.
    # Save the original shader alongside the reordered original texture so that
    # future Blender updates cannot silently change the implementation contract.
    shader = gpu.getShaderText()
    for token in ("0.55937113331007726", "0.0400000028", "0.498957247", "0.00303993467"):
        assert token in shader, "Blender OCIO transform changed; inspect before export"
    log_scale, log_offset = .0400000028, .498957247
    return {"file": path.name, "edge": edge, "channels": 4, "format": "little-endian float32 RGBA",
            "order": "RGBA vec4; red/x fastest, green/y next, blue/z slowest; index=(blue*edge+green)*edge+red; alpha=1",
            "input": "Apply the input matrix to scene-linear Rec.709, then log2 and scale/offset before tetrahedral interpolation.",
            "domain_log2": [-log_offset / log_scale, (1 - log_offset) / log_scale],
            "input_matrix_columns": [[.55937113331007726, .07622070548350629, .065526711778770175],
                                     [.3047833466230091, .78797177010284625, .16454675460372409],
                                     [.13584556048047705, .13580746617472153, .76992650165037446]],
            "log_minimum": 1.17549435e-38, "log_scale": log_scale, "log_offset": log_offset,
            "interpolation": "tetrahedral", "post_lut_power": 2.4,
            "display_conversion": {"breakpoint": .00303993467, "slope": 12.9232101,
                                   "scale": 1.05499995, "offset": .0549999997, "power": .416666657},
            "output": "LUT values require post_lut_power and display_conversion to obtain display-encoded sRGB.",
            "sample": "Use the complete exported OCIO operation chain; this is Blender's original LUT, not a sampled copy of the whole display transform.",
            "reference_shader": reference_path.name,
            "source_config_sha256": hashlib.sha256(config_path.read_bytes()).hexdigest(),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "black_display_rgb": cpu.applyRGB([0., 0., 0.]),
            "reference_samples": [{"linear": p, "display": cpu.applyRGB(p)} for p in
                                  [[.18]*3, [1.]*3, [10., .01, .01], [.01, 10., .01], [.01, .01, 10.]]]}


def export_contract(scene, original_sha):
    ggx = json.loads((ASSETS / "GemPanels_cycles_ggx.json").read_text())
    lights = []
    for obj in scene.objects:
        if obj.type != "LIGHT":
            continue
        light, matrix = obj.data, obj.matrix_world
        area = light.size * light.size_y
        lights.append({"name": obj.name, "type": light.type, "shape": light.shape,
                       "position_m": list(matrix.translation), "axis_u": list(matrix.to_3x3() @ Vector((1, 0, 0))),
                       "axis_v": list(matrix.to_3x3() @ Vector((0, 1, 0))),
                       "emission_normal": list(matrix.to_3x3() @ Vector((0, 0, -1))),
                       "size_m": [light.size, light.size_y], "power_watts": light.energy,
                       "color_linear_rgb": list(light.color),
                       "radiance_watts_per_sr_m2_rgb": [c * light.energy / (math.pi * area) for c in light.color],
                       "normalize": bool(getattr(light, "normalize", True)),
                       "spread_radians": light.spread})
    materials = []
    for obj in scene.objects:
        if not obj.name.startswith("GemPanel_") or obj.type != "MESH":
            continue
        mat = obj.data.materials[0]
        principled = next(n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
        volume = next(n for n in mat.node_tree.nodes if n.type == "VOLUME_ABSORPTION")
        materials.append({"asset": obj.name, "material": mat.name, "node_graph": nodes(mat),
                          "ior": principled.inputs["IOR"].default_value,
                          "base_color_linear_rgba": list(principled.inputs["Base Color"].default_value),
                          "roughness": principled.inputs["Roughness"].default_value,
                          "transmission": principled.inputs["Transmission Weight"].default_value,
                          "ggx_multiscatter": ggx,
                          "sigma_per_source_unit": list(mat["source_sigma_per_local_unit"]),
                          "authored_density_per_m": volume.inputs["Density"].default_value,
                          "absorption_color": list(volume.inputs["Color"].default_value),
                          "width_m": obj["panel_width_m"], "height_m": obj["panel_height_m"],
                          "sigma_per_m_rule": "sigma_per_source_unit / (0.5 * min(width_m,height_m))"})
    background = scene.world.node_tree.nodes.get("Background")
    floor = bpy.data.objects["StudioFloor"]
    floor_bsdf = floor.data.materials[0].node_tree.nodes.get("Principled BSDF")
    camera = scene.camera
    result = {"schema_version": 2, "source_blend": "GemPanels.blend", "source_sha256": original_sha,
              "parametric_blend": "GemPanels_parametric.blend", "blender_version": bpy.app.version_string,
              "coordinate_system": "Blender right-handed: X right, Y along table, Z outward; metres; table centre Z=0",
              "runtime_gltf_transform": "(x,y,z)_blender => (x,z,-y)_gltf",
              "cut": {"halfspaces": 26, "depth": .85, "crown_inset": .31, "table_z": .30,
                      "girdle_clip": .18, "pavilion_rotation_radians": math.pi / 8,
                      "pavilion_slope_fraction": .86, "pavilion_shoulder_z": -.07,
                      "source_unit_m_rule": "0.5 * min(width_m,height_m)",
                      "deformation": "Re-intersect the 26 planes; never nonuniformly stretch a finished cut.",
                      "plane_function_source": "make_gem_panels.py:planes",
                      "vertex_constraints": ["source_plane_a", "source_plane_b", "source_plane_c"]},
              "materials": materials, "lights": lights,
              "world": {"color_linear_rgb": list(background.inputs["Color"].default_value)[:3],
                        "strength": background.inputs["Strength"].default_value},
              "camera": {"name": camera.name, "type": camera.data.type,
                         "position_m": list(camera.matrix_world.translation),
                         "axis_right": list(camera.matrix_world.to_3x3() @ Vector((1, 0, 0))),
                         "axis_up": list(camera.matrix_world.to_3x3() @ Vector((0, 1, 0))),
                         "view_direction": list(camera.matrix_world.to_3x3() @ Vector((0, 0, -1))),
                         "ortho_scale_m": camera.data.ortho_scale, "lens_mm": camera.data.lens,
                         "resolution": [scene.render.resolution_x, scene.render.resolution_y]},
              "ground": {"point_m": list(floor.location), "normal": [0, 0, 1],
                         "base_color_linear_rgb": list(floor_bsdf.inputs["Base Color"].default_value)[:3],
                         "roughness": floor_bsdf.inputs["Roughness"].default_value},
              "view": {"transform": scene.view_settings.view_transform, "look": scene.view_settings.look,
                       "exposure_stops": scene.view_settings.exposure, "gamma": scene.view_settings.gamma,
                       "display": scene.display_settings.display_device, "lut": export_lut(scene)},
              "cycles": {"max_bounces": scene.cycles.max_bounces, "transmission_bounces": scene.cycles.transmission_bounces,
                         "glossy_bounces": scene.cycles.glossy_bounces, "samples": scene.cycles.samples,
                         "use_denoising": scene.cycles.use_denoising},
              "limitations": "A real-time renderer must implement the dielectric/volume transport, studio and view transform. This export does not establish Cycles pixel parity."}
    (ASSETS / "GemPanels_runtime.json").write_text(json.dumps(result, indent=2) + "\n")


def validate_dimensions(objects):
    report = []
    sizes = [(.96, .45), (.30, .66), (.40, .30), (.96, .12), (.50, .50), (.12, .96), (1.8, .18), (.18, 1.8)]
    depsgraph = bpy.context.evaluated_depsgraph_get()
    for obj in objects:
        saved = obj["panel_width_m"], obj["panel_height_m"]
        for width, height in sizes:
            obj["panel_width_m"], obj["panel_height_m"] = width, height
            obj.update_tag()
            bpy.context.view_layer.update()
            evaluated = obj.evaluated_get(depsgraph)
            mesh = evaluated.to_mesh()
            scale = min(width, height) * .5
            boundaries = source.planes(width, height)
            error = max(n.dot(v.co / scale + Vector((0, 0, .30))) - d
                        for v in mesh.vertices for n, d in boundaries)
            extent = [max(v.co[i] for v in mesh.vertices) - min(v.co[i] for v in mesh.vertices) for i in range(3)]
            assert error < 1e-4, (obj.name, width, height, error)
            assert abs(extent[0] - width) < 1e-5 and abs(extent[1] - height) < 1e-5, (obj.name, extent)
            assert all(face.normal.dot(boundaries[obj.data.attributes["source_plane"].data[face.index].value][0].normalized()) > .999
                       for face in mesh.polygons), (obj.name, width, height, "normal")
            report.append({"object": obj.name, "width": width, "height": height,
                           "max_source_plane_error": error, "dimensions": extent})
            evaluated.to_mesh_clear()
        obj["panel_width_m"], obj["panel_height_m"] = saved
        obj.update_tag()
        bpy.context.view_layer.update()
    (ASSETS / "GemPanels_deformation_checks.json").write_text(json.dumps(report, indent=2) + "\n")
    return len(report)


def main():
    original_sha = hashlib.sha256((ASSETS / "GemPanels.blend").read_bytes()).hexdigest()
    graph = recut_group(plane_group())
    objects = [o for o in bpy.context.scene.objects if o.type == "MESH" and o.name.startswith("GemPanel_")]
    for obj in objects:
        parametrize(obj, graph)
    checks = validate_dimensions(objects)
    export_contract(bpy.context.scene, original_sha)
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(ASSETS / "GemPanels_parametric.blend"))
    print("GEM_EXPORT_OK=" + json.dumps({"assets": len(objects), "dimension_checks": checks,
                                        "shader_contract": "GemPanels_runtime.json", "lut_edge": 57}))


if __name__ == "__main__":
    main()
