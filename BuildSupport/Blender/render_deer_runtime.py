"""Render actual Rust-evaluated deer IK/gaze skin, exported by the registered test.
Blender --background --python this_file; poses live under output/deer-ik-validation-20261108.
"""
import bpy, json, math, os
from pathlib import Path
from mathutils import Vector
ROOT=Path(__file__).resolve().parents[2];CHECK=Path(os.environ.get('WORMWOOD_DEER_RENDER_DIR',ROOT/'output/deer-ik-validation-20261108'))
files=sorted((CHECK/'runtime-poses').glob('[0-9][0-9].json'))
assert files,'Export evaluated poses with WORMWOOD_DEER_RUNTIME_PREVIEW_DIR first'
for o in list(bpy.data.objects):bpy.data.objects.remove(o,do_unlink=True)
scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.samples=12
scene.render.resolution_x=560;scene.render.resolution_y=480;scene.render.resolution_percentage=100
scene.world.color=(.10,.12,.14)
def point(o,p):o.rotation_euler=(Vector(p)-o.location).to_track_quat('-Z','Y').to_euler()
mid=json.loads(files[len(files)//2].read_text());side=1 if mid['pose']['look_yaw']>=0 else -1
bpy.ops.object.camera_add(location=(4,side*5,2.9));camera=bpy.context.object;point(camera,(0,0,1.35));camera.data.type='ORTHO';camera.data.ortho_scale=3.8;scene.camera=camera
for loc,power,size in [((2,side*4,6),700,5),((-3,-side*2,4),450,4)]:
 bpy.ops.object.light_add(type='AREA',location=loc);o=bpy.context.object;o.data.energy=power;o.data.size=size;point(o,(0,0,1.2))
bpy.ops.mesh.primitive_plane_add(size=200, location=(0,0,0))
ground=bpy.context.object;ground.name='Contact reference ground'
m=bpy.data.materials.new('Ground');m.diffuse_color=(.16,.20,.14,1);ground.data.materials.append(m)
materials={};skin=None
out=CHECK/'runtime-renders';out.mkdir(parents=True,exist_ok=True)
for index,path in enumerate(files):
 data=json.loads(path.read_text());rows=data['vertices']
 coords=[(-v[2],v[0],v[1])for v in rows]
 if skin is None:
  mesh=bpy.data.meshes.new('Evaluated deer skin');mesh.from_pydata(coords,[],[tuple(range(i,i+3))for i in range(0,len(coords),3)]);mesh.update()
  skin=bpy.data.objects.new('Runtime_Deer_IK_Gaze',mesh);bpy.context.collection.objects.link(skin)
  for i,p in enumerate(mesh.polygons):
   rgb=tuple(round(v,5)for v in rows[i*3][3:6])
   if rgb not in materials:
    m=bpy.data.materials.new(f'Authored RGB {len(materials)}');m.diffuse_color=(*rgb,1);m.use_nodes=True;m.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value=(*rgb,1);m.node_tree.nodes['Principled BSDF'].inputs['Roughness'].default_value=.8
    materials[rgb]=len(mesh.materials);mesh.materials.append(m)
   p.material_index=materials[rgb]
 else:
  for v,p in zip(skin.data.vertices,coords):v.co=p
  skin.data.update()
 scene.render.filepath=str(out/f'{index:02d}.png');bpy.ops.render.render(write_still=True)
 if index==len(files)//2:
  scene.render.resolution_x=900;scene.render.resolution_y=760;scene.cycles.samples=24;scene.render.filepath=str(CHECK/'Runtime_Look_Back.png');bpy.ops.render.render(write_still=True)
  scene.render.resolution_x=560;scene.render.resolution_y=480;scene.cycles.samples=12
