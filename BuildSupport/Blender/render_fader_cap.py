"""Bake the top-down mixer cap once from the original GemFadersMeters.blend."""
import array
import sys
import tempfile
from pathlib import Path

import bpy

scene = bpy.context.scene
slider = bpy.data.objects['GemFader_Ivory_Slider']
for obj in scene.objects:
    if obj.type == 'MESH' and obj.parent != slider:
        # Let the original studio reflect in the jewel without painting it
        # into the transparent background.
        obj.visible_camera = False
camera = scene.camera
center = slider.matrix_world.translation
camera.location = (center.x, center.y, center.z + 0.3)
camera.rotation_euler = (0., 0., 0.)
camera.data.type = 'ORTHO'
camera.data.ortho_scale = 0.025
scene.render.engine = 'CYCLES'
scene.cycles.samples = 96
scene.render.film_transparent = True
scene.render.resolution_x = scene.render.resolution_y = 128
scene.render.resolution_percentage = 100
scene.render.image_settings.file_format = 'OPEN_EXR'
scene.render.image_settings.color_mode = 'RGBA'
scene.render.image_settings.color_depth = '32'
with tempfile.TemporaryDirectory() as directory:
    scene.render.filepath = str(Path(directory) / 'cap.exr')
    bpy.ops.render.render(write_still=True)
    image = bpy.data.images.load(scene.render.filepath, check_existing=False)
    samples = array.array('f', [0.]) * (128 * 128 * 4)
    image.pixels.foreach_get(samples)
    if sys.byteorder != 'little':
        samples.byteswap()
    dest = Path(bpy.data.filepath).parent / 'ivory_cap_linear.rgba32f'
    dest.write_bytes(samples.tobytes())
