import bpy, math
from mathutils import Vector
from pathlib import Path
root=Path(__file__).resolve().parents[2]
s=bpy.context.scene;s.render.engine='CYCLES';s.cycles.samples=24;s.render.resolution_x=800;s.render.resolution_y=650;s.render.resolution_percentage=100
s.world.color=(.16,.16,.16)
rig=bpy.data.objects['Cardinal_Rig']
def point(o,target):o.rotation_euler=(Vector(target)-o.location).to_track_quat('-Z','Y').to_euler()
bpy.ops.object.camera_add(location=(-.40,-.48,.28));cam=bpy.context.object;point(cam,(0,0,.12));cam.data.type='ORTHO';cam.data.ortho_scale=.40;s.camera=cam
for loc,power,size in [((-.3,-.4,.5),15,.4),((.2,.3,.4),10,.3)]:
 bpy.ops.object.light_add(type='AREA',location=loc);o=bpy.context.object;o.data.energy=power;o.data.shape='DISK';o.data.size=size;point(o,(0,0,.12))
for clip,frame in [('Perch',1),('Flight',5),('Flight',13),('Call',3)]:
 rig.animation_data.action=bpy.data.actions['Cardinal_'+clip];s.frame_set(frame);s.render.filepath=str(root/'Art/Birds/Cardinal'/f'{clip}_{frame}.png');bpy.ops.render.render(write_still=True)
# Optional slow-motion flight preview: 16 authored phases at 25 fps (4x slow).
import sys
if '--flight-animation' in sys.argv:
 s.render.resolution_x=480;s.render.resolution_y=400;s.cycles.samples=12
 folder=root/'output/checks/cardinal/flight-frames';folder.mkdir(parents=True,exist_ok=True)
 rig.animation_data.action=bpy.data.actions['Cardinal_Flight']
 for frame in range(1,17):
  s.frame_set(frame);s.render.filepath=str(folder/f'{frame:02d}.png');bpy.ops.render.render(write_still=True)
