"""Check saved parametric assets and display LUT, without rendering or saving art.

Blender --background --disable-autoexec <GemPanels_parametric.blend>
    --python BuildSupport/Blender/check_gem_panels.py
"""
import array
import json
import math
import random
from pathlib import Path

import bpy
import PyOpenColorIO as ocio

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "Art/AudioWorkstation/GemPanels"
contract = json.loads((ASSETS / "GemPanels_runtime.json").read_text())
checks = []
for obj in bpy.context.scene.objects:
    if obj.type != "MESH" or not obj.name.startswith("GemPanel_"):
        continue
    for frame, width, height in ((1, .18, 1.8), (11, 1.8, .18)):
        obj["panel_width_m"], obj["panel_height_m"] = width, height
        obj.keyframe_insert(data_path='["panel_width_m"]', frame=frame)
        obj.keyframe_insert(data_path='["panel_height_m"]', frame=frame)
    for frame in (1, 3, 6, 9, 11):
        bpy.context.scene.frame_set(frame)
        bpy.context.view_layer.update()
        depsgraph = bpy.context.evaluated_depsgraph_get()
        width, height = obj["panel_width_m"], obj["panel_height_m"]
        evaluated = obj.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh()
        dimensions = [max(v.co[i] for v in mesh.vertices) - min(v.co[i] for v in mesh.vertices) for i in range(3)]
        assert abs(dimensions[0] - width) < 1e-5 and abs(dimensions[1] - height) < 1e-5
        assert abs(dimensions[2] - .575 * min(width, height)) < 1e-5
        mat = evaluated.data.materials[0]
        density = next(n for n in mat.node_tree.nodes if n.type == "VOLUME_ABSORPTION").inputs["Density"].default_value
        expected_density = max(mat["source_sigma_per_local_unit"]) / (.5 * min(width, height))
        assert abs(density - expected_density) < .001
        evaluated.to_mesh_clear()
        checks.append({"object": obj.name, "frame": frame, "dimensions_m": dimensions,
                       "absorption_density_per_m": density})

config = ocio.Config.CreateFromFile(str(Path(bpy.utils.resource_path("LOCAL")) / "datafiles/colormanagement/config.ocio"))
cpu = config.getProcessor(ocio.DisplayViewTransform(src=config.getRoleColorSpace("scene_linear"),
                                                   display="sRGB", view="AgX")).getDefaultCPUProcessor()
lut = contract["view"]["lut"]
data = array.array("f")
data.frombytes((ASSETS / lut["file"]).read_bytes())
edge = lut["edge"]
low, high = lut["domain_log2"]
assert len(data) == edge ** 3 * 4
assert all(data[i] == 1 for i in range(3, len(data), 4))


def sample(rgb):
    columns = lut["input_matrix_columns"]
    mixed = [sum(columns[c][r] * rgb[c] for c in range(3)) for r in range(3)]
    coord = [min(edge - 1., max(0., (math.log2(max(v, lut["log_minimum"])) * lut["log_scale"] + lut["log_offset"]) * (edge - 1))) for v in mixed]
    base = [min(edge - 2, int(v)) for v in coord]
    fraction = [coord[i] - base[i] for i in range(3)]
    order = sorted(range(3), key=lambda i: fraction[i], reverse=True)
    offsets = [[0, 0, 0]]
    for axis in order:
        offset = offsets[-1].copy()
        offset[axis] += 1
        offsets.append(offset)
    x, y, z = [fraction[i] for i in order]
    weights = [1 - x, x - y, y - z, z]
    result = [0., 0., 0.]
    for offset, weight in zip(offsets, weights):
        r, g, b = [base[i] + offset[i] for i in range(3)]
        i = ((b * edge + g) * edge + r) * 4
        for c in range(3):
            result[c] += weight * data[i + c]
    linear = [max(0., v) ** lut["post_lut_power"] for v in result]
    display = lut["display_conversion"]
    return [v * display["slope"] if v <= display["breakpoint"] else
            v ** display["power"] * display["scale"] - display["offset"] for v in linear]


random.seed(137)
errors = []
for _ in range(1000):
    rgb = [2 ** random.uniform(low, high) for _ in range(3)]
    reference = cpu.applyRGB(rgb)
    approximate = sample(rgb)
    errors.append(max(abs(a - b) for a, b in zip(reference, approximate)))
assert max(errors) < 2e-5, max(errors)
report = {"saved_asset_animation_checks": checks, "python_autoexec_enabled": bpy.context.preferences.filepaths.use_scripts_auto_execute,
          "lut_comparison_samples": len(errors), "lut_max_display_channel_error": max(errors),
          "lut_mean_max_channel_error": sum(errors) / len(errors),
          "limitations": "Numeric asset and color-transform checks only. No rendered appearance or native device is verified by this script."}
(ASSETS / "GemPanels_runtime_checks.json").write_text(json.dumps(report, indent=2) + "\n")
print("GEM_RUNTIME_CHECK_OK=" + json.dumps({"animation_checks": len(checks), "lut_samples": len(errors),
                                          "lut_max_display_channel_error": max(errors)}))
