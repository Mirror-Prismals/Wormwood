"""Offline Cycles RGBA flipbooks from the authoritative BubbleNotes.blend.

Run Blender --background <BubbleNotes.blend> --python this_file -- --output DIR.
No application/startup path calls this authoring command. Preserve source optics;
Cycles transparent-glass film supplies the transmitted-background alpha.
"""
from pathlib import Path
import argparse, hashlib, json, math, sys
import bpy
import numpy as np
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--samples', type=int, default=64)
parser.add_argument('--probe', action='store_true')
args = parser.parse_args(sys.argv[sys.argv.index('--')+1:])
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
scene = bpy.context.scene
assert scene.name == 'Soap Bubble Piano Notes'
source = ROOT/'Art/AudioWorkstation/BubbleNotes/BubbleNotes.blend'
source_hash = hashlib.sha256(source.read_bytes()).hexdigest()

# Keep original Principled material nodes, time drivers, world and studio lights.
# The floor belongs to the authoring preview, not to an independent UI note.
objects = list(bpy.data.collections['01_Bubble_Note_Assets'].objects)
original_locations = {o.name:o.location.copy() for o in objects}
for obj in objects:
    obj.animation_data_clear()
    obj.hide_render = True
    obj.data.shape_keys.animation_data_clear()
    for key in obj.data.shape_keys.key_blocks:
        key.value = 0.
studio = bpy.data.collections['02_Plain_Background_And_Studio']
lights = [(o,o.location.copy()) for o in studio.objects if o.type=='LIGHT']
bpy.data.objects['Plain_Background'].hide_render = True
scene.render.film_transparent = True
scene.cycles.film_transparent_glass = True
scene.cycles.film_transparent_roughness = .1
scene.cycles.samples = args.samples
scene.cycles.use_denoising = True
scene.cycles.use_animated_seed = False
scene.cycles.seed = 17
scene.render.use_persistent_data = True
scene.render.image_settings.file_format = 'OPEN_EXR'
scene.render.image_settings.color_mode = 'RGBA'
scene.render.image_settings.color_depth = '32'
try:
    prefs = bpy.context.preferences.addons['cycles'].preferences
    prefs.compute_device_type = 'METAL'
    prefs.get_devices()
    if any(d.type == 'METAL' for d in prefs.devices):
        for device in prefs.devices:
            device.use = device.type == 'METAL'
        scene.cycles.device = 'GPU'
except Exception as error:
    print('Cycles device fallback:',error,flush=True)

fps = 12
placement_frames = 18
idle_frames = 32
frames_per_family = placement_frames+idle_frames
families = []
raw = out/'images.rgba8'
raw.write_bytes(b'')
offset = 0
for suffix,length in [('Short',.0225),('Medium',.045),('Long',.09),('Sustained',.18)]:
    obj = bpy.data.objects['BubbleNote_'+suffix]
    obj.hide_render = False
    obj.location=(0,0,0)
    for light,location in lights:
        light.location=location-original_locations[obj.name]
    pad=.0015
    low_y,high_y=-.016,.016
    height=192
    width=int(math.ceil((length+pad*2)/(high_y-low_y)*height/8))*8
    pad=((high_y-low_y)*width/height-length)*.5
    scene.render.resolution_x=width
    scene.render.resolution_y=height
    scene.render.resolution_percentage=100
    camera=scene.camera
    camera.location=(length*.5,-.12,.30)
    camera.rotation_euler=(Vector((length*.5,0,.003))-camera.location).to_track_quat('-Z','Y').to_euler()
    camera.data.ortho_scale=(high_y-low_y)*width/height
    # Match the source camera's projected volume without stretching one Short
    # image into every duration. Runtime preserves these rounded shoulders.
    family={'name':suffix,'length':length,'pad':pad,'low_y':low_y,'high_y':high_y,
            'width':width,'height':height,'offset':offset,'frames':frames_per_family}
    pngs=out/'frames'/suffix
    pngs.mkdir(parents=True,exist_ok=True)
    reflection=bpy.data.images.new('Baked reflection '+suffix,width=width,height=height,alpha=True,float_buffer=True)
    for index in range(1 if args.probe else frames_per_family):
        time=min(index/fps,1.35) if index<placement_frames else 1.35+(index-placement_frames)/fps
        scene.frame_set(1)
        scene.frame_set(int(time*24)+1,subframe=(time*24)%1)
        obj.location=(0,0,0)
        squash=obj.data.shape_keys.key_blocks['Plop_Squash_Stretch']
        wave=obj.data.shape_keys.key_blocks['Settling_Surface_Wave']
        squash.value=wave.value=0.
        if time<.125:
            obj.location.z=.009*(1-time/.125)**2
        elif time<1.35:
            tau=time-.125
            squash.value=math.exp(-6*tau)*math.cos(20*tau)
            wave.value=math.exp(-4.9*tau)*math.sin(25*tau)
        image=pngs/f'{index:03d}.exr'
        scene.render.image_settings.file_format='OPEN_EXR'
        scene.render.image_settings.color_depth='32'
        scene.render.filepath=str(image)
        bpy.ops.render.render(write_still=True)
        # EXR contains premultiplied scene-linear reflected light. Preserve its
        # energy BEFORE grading: straight PNG conversion clips E/alpha and then
        # blending by alpha again makes bright glass reflections disappear.
        loaded=bpy.data.images.load(str(image),check_existing=False)
        pixels=np.empty(len(loaded.pixels),dtype=np.float32)
        loaded.pixels.foreach_get(pixels)
        pixels=pixels.reshape(height,width,4)
        alpha=pixels[:,:,3].copy()
        opaque=pixels.copy();opaque[:,:,3]=1.
        reflection.pixels.foreach_set(opaque.ravel())
        scene.render.image_settings.file_format='PNG'
        scene.render.image_settings.color_depth='8'
        reflection.save_render(str(pngs/f'{index:03d}.png'),scene=scene)
        display=bpy.data.images.load(str(pngs/f'{index:03d}.png'),check_existing=False)
        display.colorspace_settings.name='Non-Color'
        rgba=np.empty(len(display.pixels),dtype=np.float32)
        display.pixels.foreach_get(rgba)
        rgba=rgba.reshape(height,width,4);rgba[:,:,3]=alpha
        encoded=np.round(np.clip(np.flipud(rgba),0,1)*255).astype(np.uint8)
        bpy.data.images.remove(display)
        assert np.all(np.isfinite(pixels))
        with raw.open('ab') as file:
            file.write(encoded.tobytes())
        bpy.data.images.remove(loaded)
        if index!=placement_frames:image.unlink()
        offset+=width*height
        print(f'BUBBLE_FRAME {suffix} {index+1}/{frames_per_family}',flush=True)
    bpy.data.images.remove(reflection)
    families.append(family)
    obj.hide_render=True
    if args.probe:
        break

manifest={'schema':2,'source':'Art/AudioWorkstation/BubbleNotes/BubbleNotes.blend',
          'source_sha256':source_hash,'encoding':'premultiplied display reflection RGB + transmitted-background occlusion alpha, RGBA8 top row first',
          'alpha':'Cycles transparent glass; composite reflected light + (1-alpha)*background, never constant opacity',
          'fps':fps,'placement_frames':placement_frames,'idle_frames':idle_frames,
          'loop_crossfade_frames':4,'settle_seconds':1.35,'families':families,
          'bytes':raw.stat().st_size,'images_sha256':hashlib.sha256(raw.read_bytes()).hexdigest(),
          'samples':args.samples,'device':scene.cycles.device,
          'policy':'Offline source-material rendering. Runtime image selection/interpolation only; no optical recapture.'}
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
assert hashlib.sha256(source.read_bytes()).hexdigest()==source_hash
print('BUBBLE_ART_COMPLETE',json.dumps(manifest),flush=True)
