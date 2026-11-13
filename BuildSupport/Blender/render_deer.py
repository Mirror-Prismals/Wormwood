"""Render authored deer poses; optional --run-animation gives a slow preview."""
import bpy, math, sys
from pathlib import Path
from mathutils import Vector
root=Path(__file__).resolve().parents[2];folder=root/'Art/Wildlife/Deer'
s=bpy.context.scene;s.render.engine='CYCLES';s.cycles.samples=24;s.render.resolution_x=850;s.render.resolution_y=700;s.render.resolution_percentage=100
s.world.color=(.12,.14,.16)
rig=bpy.data.objects['Whitetail_Rig']
def point(o,p):o.rotation_euler=(Vector(p)-o.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add(location=(-4.,-6.,2.8));cam=bpy.context.object;point(cam,(0,0,1.35));cam.data.type='ORTHO';cam.data.ortho_scale=3.7;s.camera=cam
for loc,power,size in [((-3,-4,6),600,5),((3,3,5),450,4)]:
 bpy.ops.object.light_add(type='AREA',location=loc);o=bpy.context.object;o.data.energy=power;o.data.shape='DISK';o.data.size=size;point(o,(0,0,1.2))
for clip,frame in [('Idle',1),('Alert',1),('Run',1),('Run',7),('Run',13),('Run',19)]:
 rig.animation_data.action=bpy.data.actions['Deer_'+clip];s.frame_set(frame);s.render.filepath=str(folder/f'{clip}_{frame}.png');bpy.ops.render.render(write_still=True)
if '--run-animation' in sys.argv:
 s.cycles.samples=12;s.render.resolution_x=480;s.render.resolution_y=400
 out=root/'output/checks/deer/run-frames';out.mkdir(parents=True,exist_ok=True)
 rig.animation_data.action=bpy.data.actions['Deer_Run']
 for frame in range(1,25):
  s.frame_set(frame);s.render.filepath=str(out/f'{frame:02d}.png');bpy.ops.render.render(write_still=True)
