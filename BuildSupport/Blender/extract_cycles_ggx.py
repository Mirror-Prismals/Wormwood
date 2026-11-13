"""Slice Cycles' exact energy tables for the currently authored gem material.

Usage: python3 BuildSupport/Blender/extract_cycles_ggx.py path/to/shader.tables
Input is Blender v5.0.1 intern/cycles/scene/shader.tables (Apache-2.0).
This is a 16-point angular slice, not a replacement fitted optical model.
"""
import hashlib
import json
import math
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[2]
path = Path(sys.argv[1])
text = path.read_text()
roughness, ior = .017999999225139618, 1.7660000324249268
x = roughness * 15
z = math.sqrt((ior - 1) / (ior + 1)) * 15
x0, z0 = int(x), int(z)
fx, fz = x - x0, z - z0
curves = {}
for name in ("ggx_glass_E", "ggx_glass_inv_E"):
    body = re.search(r"table_" + name + r"\[4096\] = \{(.*?)\};", text, re.S).group(1)
    values = [float(v) for v in re.findall(r"([-+]?\d*\.?\d+(?:e[-+]?\d+)?)f", body)]
    assert len(values) == 4096
    samples = []
    for mu in range(16):
        value = 0.
        for dz, wz in ((0, 1 - fz), (1, fz)):
            for dx, wx in ((0, 1 - fx), (1, fx)):
                value += wz * wx * values[((z0 + dz) * 16 + mu) * 16 + x0 + dx]
        samples.append(value)
    curves[name] = samples
result = {"source": "https://raw.githubusercontent.com/blender/blender/v5.0.1/intern/cycles/scene/shader.tables",
          "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
          "copyright": "2011-2022 Blender Foundation", "license": "Apache-2.0",
          "modification": "Bilinearly sliced the original 16-cube at authored roughness and IOR; retained 16 original mu stations.",
          "roughness": roughness, "ior": ior, "mu": [i / 15 for i in range(16)],
          "interpolation": "Clamp mu to[0,1], linearly interpolate adjacent values at mu*15; energy_scale=1/E.",
          "ggx_glass_E": curves["ggx_glass_E"], "ggx_glass_inv_E": curves["ggx_glass_inv_E"]}
output = root / "Art/AudioWorkstation/GemPanels/GemPanels_cycles_ggx.json"
output.write_text(json.dumps(result, indent=2) + "\n")
print(output)
