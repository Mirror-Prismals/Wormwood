"""Build/render a Cycles reference from the native workstation's exact frame.

Blender --background --factory-startup --python <this-file> -- \
    /absolute/workstation-optical-frame.json /absolute/reference.png [--prepare-only]

Default is a <=640px, 32-sample render. Run through a parent CPJ job; rendering
can take more than one minute. --prepare-only saves the matched .blend without
rendering so the same file can be rendered later. Authored art is not modified.
"""
import argparse
import itertools
import json
import math
from pathlib import Path
import sys

import bpy
from mathutils import Matrix, Vector


def parse():
    parser = argparse.ArgumentParser()
    parser.add_argument("frame", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--samples", type=int, default=32)
    parser.add_argument("--max-size", type=int, default=640)
    parser.add_argument("--no-denoise", action="store_true",
                        help="Keep the raw Cycles samples for an unbiased native comparison")
    parser.add_argument("--strict-transport", action="store_true",
                        help="Disable glossy filtering, sample clamping and adaptive sampling; use a one-pixel box filter. Pair with --no-denoise for raw output.")
    parser.add_argument("--bounces", type=int,
                        help="Set total, glossy and transmission limits together; omitted preserves authored 16/12/12")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:])
    if args.samples < 1 or args.max_size < 1:
        parser.error("samples and max-size must be positive")
    if args.bounces is not None and not 1 <= args.bounces <= 1024:
        parser.error("bounces must be in [1,1024]")
    return args


def solid(planes):
    boundaries = [(Vector(p[:3]), p[3]) for p in planes]
    vertices = []
    for (a, da), (b, db), (c, dc) in itertools.combinations(boundaries, 3):
        determinant = a.dot(b.cross(c))
        if abs(determinant) < 1e-8:
            continue
        p = (da * b.cross(c) + db * c.cross(a) + dc * a.cross(b)) / determinant
        if any(n.dot(p) > d + 3e-7 for n, d in boundaries):
            continue
        if not any((p - q).length < 2e-6 for q in vertices):
            vertices.append(p)
    faces = []
    for normal, distance in boundaries:
        indices = [i for i, p in enumerate(vertices) if abs(normal.dot(p) - distance) < 5e-7]
        if len(indices) < 3:
            continue
        center = sum((vertices[i] for i in indices), Vector()) / len(indices)
        n = normal.normalized()
        tangent = n.cross(Vector((0, 0, 1)) if abs(n.z) < .9 else Vector((0, 1, 0))).normalized()
        bitangent = n.cross(tangent)
        indices.sort(key=lambda i: math.atan2((vertices[i] - center).dot(bitangent), (vertices[i] - center).dot(tangent)))
        faces.append(indices)
    assert len(vertices) >= 8 and len(faces) >= 6, "Invalid exported panel halfspaces"
    return vertices, faces


def orient(obj, right, up, back, origin):
    matrix = Matrix((Vector(right), Vector(up), Vector(back))).transposed().to_4x4()
    matrix.translation = Vector(origin)
    obj.matrix_world = matrix


def render_controls(scene):
    """Record controls that otherwise make reference comparisons ambiguous."""
    cycles = scene.cycles
    return {
        "glossy_filter": cycles.blur_glossy,
        "clamp_direct": cycles.sample_clamp_direct,
        "clamp_indirect": cycles.sample_clamp_indirect,
        "adaptive_sampling": cycles.use_adaptive_sampling,
        "adaptive_threshold": cycles.adaptive_threshold,
        "adaptive_min_samples": cycles.adaptive_min_samples,
        "pixel_filter": cycles.pixel_filter_type,
        "pixel_filter_width": cycles.filter_width,
        "reflective_caustics": cycles.caustics_reflective,
        "refractive_caustics": cycles.caustics_refractive,
        "denoising": cycles.use_denoising,
    }


def main():
    args = parse()
    payload = json.loads(args.frame.read_text())
    rows = payload["rows"]
    assert len(rows) >= 256 and all(len(r) == 4 for r in rows)
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    scene = bpy.context.scene
    factory_controls = render_controls(scene)
    for i in range(int(rows[0][1])):
        at = 32 + i * 40
        if rows[at][0] <= 0:
            continue
        vertices, faces = solid(rows[at + 6:at + 32])
        mesh = bpy.data.meshes.new("Panel_%d_exact_planes" % i)
        mesh.from_pydata(vertices, [], faces)
        mesh.update()
        obj = bpy.data.objects.new("Panel_%d" % i, mesh)
        scene.collection.objects.link(obj)
        material = bpy.data.materials.new("Panel_%d_authored_dielectric" % i)
        material.use_nodes = True
        nodes, links = material.node_tree.nodes, material.node_tree.links
        nodes.clear()
        output = nodes.new("ShaderNodeOutputMaterial")
        glass = nodes.new("ShaderNodeBsdfPrincipled")
        glass.inputs["Base Color"].default_value = (*rows[at + 4][:3], 1.)
        glass.inputs["IOR"].default_value = rows[at][1]
        glass.inputs["Roughness"].default_value = rows[at][2]
        glass.inputs["Transmission Weight"].default_value = rows[at][3]
        links.new(glass.outputs["BSDF"], output.inputs["Surface"])
        sigma = rows[at + 1][:3]
        volume = nodes.new("ShaderNodeVolumeAbsorption")
        volume.inputs["Color"].default_value = (*[1 - s / max(sigma) for s in sigma], 1.)
        volume.inputs["Density"].default_value = max(sigma)
        links.new(volume.outputs["Volume"], output.inputs["Volume"])
        mesh.materials.append(material)
    floor_z = rows[5][3]
    bpy.ops.mesh.primitive_plane_add(size=200, location=(0, 0, floor_z))
    floor = bpy.context.object
    floor.name = "Matched studio ground"
    material = bpy.data.materials.new("Matched studio charcoal")
    material.use_nodes = True
    surface = material.node_tree.nodes.get("Principled BSDF")
    surface.inputs["Base Color"].default_value = (*rows[6][:3], 1.)
    surface.inputs["Roughness"].default_value = rows[6][3]
    floor.data.materials.append(material)
    for i in range(int(rows[9][0])):
        at = 16 + i * 5
        light = bpy.data.lights.new("Matched area %d" % i, "AREA")
        light.shape = "RECTANGLE"
        light.size, light.size_y = rows[at + 1][3] * 2, rows[at + 2][3] * 2
        radiance = rows[at + 4][:3]
        maximum = max(radiance)
        light.color = [v / maximum for v in radiance]
        light.energy = maximum * math.pi * light.size * light.size_y
        light.spread = math.pi
        obj = bpy.data.objects.new(light.name, light)
        scene.collection.objects.link(obj)
        orient(obj, rows[at + 1][:3], rows[at + 2][:3], [-n for n in rows[at + 3][:3]], rows[at][:3])
    world = bpy.data.worlds.new("Matched world radiance")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (*rows[5][:3], 1.)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.
    scene.world = world
    camera = bpy.data.cameras.new("Matched head-on orthographic camera")
    camera.type = "ORTHO"
    camera.sensor_fit = "HORIZONTAL"
    camera.ortho_scale = rows[2][3] * 2
    obj = bpy.data.objects.new(camera.name, camera)
    scene.collection.objects.link(obj)
    orient(obj, rows[2][:3], rows[3][:3], [-n for n in rows[4][:3]], rows[1][:3])
    scene.camera = obj
    scene.render.engine = "CYCLES"
    scene.cycles.samples = args.samples
    scene.cycles.use_denoising = not args.no_denoise
    scene.cycles.max_bounces = args.bounces if args.bounces is not None else 16
    scene.cycles.glossy_bounces = args.bounces if args.bounces is not None else 12
    scene.cycles.transmission_bounces = args.bounces if args.bounces is not None else 12
    if args.strict_transport:
        scene.cycles.blur_glossy = 0.
        scene.cycles.sample_clamp_direct = 0.
        scene.cycles.sample_clamp_indirect = 0.
        scene.cycles.use_adaptive_sampling = False
        scene.cycles.pixel_filter_type = "BOX"
        scene.cycles.filter_width = 1.
    scene.view_settings.view_transform = "AgX"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = rows[0][3]
    scene.view_settings.gamma = 1.
    scene.display_settings.display_device = "sRGB"
    scale = min(1., args.max_size / max(rows[8][:2]))
    scene.render.resolution_x = max(1, round(rows[8][0] * scale))
    scene.render.resolution_y = max(1, round(rows[8][1] * scale))
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(args.output.resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    bpy.context.preferences.filepaths.save_version = 0
    bpy.ops.wm.save_as_mainfile(filepath=str(args.output.with_suffix(".blend").resolve()))
    print("REFERENCE_SCENE_READY=" + json.dumps({"frame": str(args.frame), "output": str(args.output),
          "resolution": [scene.render.resolution_x, scene.render.resolution_y], "samples": args.samples,
          "denoising": scene.cycles.use_denoising,
          "strict_transport": args.strict_transport,
          "factory_controls": factory_controls,
          "render_controls": render_controls(scene),
          "bounce_limits": {"total": scene.cycles.max_bounces,
                            "glossy": scene.cycles.glossy_bounces,
                            "transmission": scene.cycles.transmission_bounces}}))
    if not args.prepare_only:
        bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    main()
