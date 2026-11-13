"""Editable beveled piano keys and case; export exact convex planes for native tracing."""
import bpy, json
from pathlib import Path
from mathutils import Vector
root=Path(__file__).resolve().parents[2]
out=root/'Art/AudioWorkstation/PianoKeyboard';out.mkdir(parents=True,exist_ok=True)
bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
def material(name,color,roughness):
 m=bpy.data.materials.new(name);m.diffuse_color=(*color,1);m.use_nodes=True
 p=m.node_tree.nodes.get('Principled BSDF');p.inputs['Base Color'].default_value=(*color,1);p.inputs['Roughness'].default_value=roughness
 return m
ivory=material('Warm ivory porcelain',(0.84,0.81,0.70),.24)
ebony=material('Black ebony',(0.009,0.011,0.014),.20)
walnut=material('Dark walnut case',(0.055,0.027,0.016),.32)
def cube(name,loc,size,mat,bevel):
 bpy.ops.mesh.primitive_cube_add(size=1,location=loc);o=bpy.context.object;o.name=name;o.dimensions=size
 bpy.ops.object.transform_apply(location=False,rotation=False,scale=True)
 o.data.materials.append(mat);b=o.modifiers.new('Machined edge bevel','BEVEL');b.width=bevel;b.segments=1
 bpy.context.view_layer.objects.active=o;bpy.ops.object.modifier_apply(modifier=b.name)
 return o
# Native instances scale this exact convex solid independently along each axis.
key=cube('Key template - native convex authority',(0,0,-2),(1,1,1),ivory,.035)
planes=[]
for face in key.data.polygons:
 n=face.normal;d=n.dot(key.data.vertices[face.vertices[0]].co)
 row=[float(x) for x in (*n,d)]
 if not any(max(abs(a-b) for a,b in zip(row,q))<1e-5 for q in planes):planes.append(row)
key.hide_render=True;key.hide_set(True)
# A tangible single-octave model: long ivory keys with raised short accidentals.
for i,p in enumerate([0,2,4,5,7,9,11]):
 cube('Ivory '+['C','D','E','F','G','A','B'][i],(.0,i*.024,.0),(.145,.023,.012),ivory,.0011)
for p,y in [(1,.012),(3,.036),(6,.084),(8,.108),(10,.132)]:
 cube('Ebony '+str(p),(-.028,y,.012),(.089,.013,.015),ebony,.0009)
cube('Walnut keybed',(-.003,.072,-.012),(.164,.184,.014),walnut,.002)
cube('Walnut spine',(-.079,.072,.001),(.012,.184,.038),walnut,.0015)
bpy.ops.wm.save_as_mainfile(filepath=str(out/'PianoKeyboard.blend'))
bpy.ops.export_scene.gltf(filepath=str(out/'PianoKeyboard.glb'),export_format='GLB',use_visible=True)
(out/'PianoKeyboard_runtime.json').write_text(json.dumps({'schema':1,'planes':planes,'source':'PianoKeyboard.blend','materials':{'ivory':[.84,.81,.70],'ebony':[.009,.011,.014],'walnut':[.055,.027,.016]},'note':'Native key instances trace these exact bevel halfspaces; no painted keyboard texture.'},indent=2)+'\n')
print('Exported',len(planes),'convex key planes')
