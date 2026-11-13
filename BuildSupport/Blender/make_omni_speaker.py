"""Build the cheat-visible omnidirectional ambience emitter and runtime triangles."""
import bpy, json, math
from pathlib import Path
root=Path(__file__).resolve().parents[2]
out=root/'Art/Audio/OmniSpeaker';out.mkdir(parents=True,exist_ok=True)
bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
def material(name,color,metal):
 m=bpy.data.materials.new(name);m.diffuse_color=(*color,1);m.use_nodes=True
 bs=m.node_tree.nodes.get('Principled BSDF');bs.inputs['Base Color'].default_value=(*color,1);bs.inputs['Metallic'].default_value=metal;bs.inputs['Roughness'].default_value=.35
 return m
shell=material('Graphite spherical grille',(.10,.12,.14),.7)
rim=material('Brushed aluminium ribs',(.55,.60,.64),.8)
led=material('Amber source indicator',(.95,.35,.04),.2)
bpy.ops.mesh.primitive_uv_sphere_add(segments=20,ring_count=10,radius=.23);bpy.context.object.data.materials.append(shell)
for z in [-.17,-.085,0,.085,.17]:
 radius=math.sqrt(.23**2-z*z)
 bpy.ops.mesh.primitive_torus_add(major_segments=24,minor_segments=4,location=(0,0,z),major_radius=radius,minor_radius=.012)
 bpy.context.object.data.materials.append(rim)
bpy.ops.mesh.primitive_uv_sphere_add(segments=12,ring_count=6,radius=.045,location=(0,0,.245));bpy.context.object.data.materials.append(led)
verts=[]
for obj in bpy.context.scene.objects:
 if obj.type!='MESH':continue
 mesh=obj.data;mesh.calc_loop_triangles();c=list(mesh.materials[0].diffuse_color[:3])
 for tri in mesh.loop_triangles:
  for i in tri.vertices:
   p=obj.matrix_world@mesh.vertices[i].co
   verts.append([round(p.x,6),round(p.z,6),round(-p.y,6)]+c)
(out/'runtime.json').write_text(json.dumps(verts,separators=(',',':')))
bpy.ops.wm.save_as_mainfile(filepath=str(out/'OmniSpeaker.blend'))
print(f'Exported {len(verts)//3} triangles')
