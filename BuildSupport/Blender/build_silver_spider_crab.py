"""Author the articulated silver spider crab and export evaluated locomotion frames.
Run with Blender --background --python this_file. Does not touch other scenes.
"""
import bpy, math, struct, json
from pathlib import Path
from mathutils import Vector
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'Art/Wildlife/SilverSpiderCrab'; OUT.mkdir(parents=True,exist_ok=True)
ASSET=ROOT/'Procedures/Assets/Wildlife'; ASSET.mkdir(parents=True,exist_ok=True)
bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)
def material(name,color,metal=1,rough=.2):
 m=bpy.data.materials.new(name); m.diffuse_color=(*color,1); m.use_nodes=True
 p=m.node_tree.nodes.get('Principled BSDF'); p.inputs['Base Color'].default_value=(*color,1); p.inputs['Metallic'].default_value=metal;p.inputs['Roughness'].default_value=rough
 return m
silver=material('Polished bright silver',(.78,.84,.9)); dark=material('Recessed gunmetal joints',(.075,.095,.12),1,.32); edge=material('Mirror silver edges',(.95,.97,1),1,.13)
eye=material('Amber optical sensors',(.9,.22,.015),.35,.18)
p=eye.node_tree.nodes.get('Principled BSDF');p.inputs['Emission Color'].default_value=(1,.12,.005,1);p.inputs['Emission Strength'].default_value=2
parts=[]
def ellipsoid(name,loc,scale,mat,segments=16,rings=8):
 bpy.ops.mesh.primitive_uv_sphere_add(segments=segments,ring_count=rings,location=loc);o=bpy.context.object;o.name=name;o.scale=scale;o.data.materials.append(mat);parts.append(o);return o
def rod(name,a,b,r1,r2,mat,parent=None):
 a,b=Vector(a),Vector(b);d=b-a
 bpy.ops.mesh.primitive_cone_add(vertices=8,radius1=r1,radius2=r2,depth=d.length,location=(a+b)/2)
 o=bpy.context.object;o.name=name;o.rotation_euler=d.to_track_quat('Z','Y').to_euler();o.data.materials.append(mat);parts.append(o)
 if parent: o.parent=parent; o.matrix_parent_inverse=parent.matrix_world.inverted()
 return o
def pivot(name,pos,parent=None):
 o=bpy.data.objects.new(name,None);bpy.context.collection.objects.link(o);o.location=pos
 if parent: o.parent=parent;o.matrix_parent_inverse=parent.matrix_world.inverted()
 bpy.context.view_layer.update();return o
ellipsoid('Broad shield carapace',(0,0,1.12),(1.03,.8,.42),silver,24,12)
ellipsoid('Raised central armored keel',(0,.03,1.42),(.46,.66,.15),edge)
ellipsoid('Dark undercarriage',(0,0,.94),(.81,.65,.2),dark)
for side in [-1,1]:
 for j in range(5):
  x=side*(.88-.06*abs(j-2));y=-.59+j*.27
  rod('Carapace serration', (x,y,1.2),(x+side*.28,y-.1,1.21),.13,0,edge)
 for j in range(4):
  y=-.57+j*.37; a=(side*.77,y,1.08);b=(side*(1.48+.1*math.sin(j)),y+(j-1.5)*.21,1.42);c=(side*(2.04+.14*math.sin(j)),y+(j-1.5)*.45,.12)
  hip=pivot(f'Leg {side} {j} hip',a); knee=pivot(f'Leg {side} {j} knee',b,hip)
  o=ellipsoid('Ball joint',a,(.15,.15,.15),dark,12,6);o.parent=hip;o.matrix_parent_inverse=hip.matrix_world.inverted()
  rod('Silver upper leg',a,b,.13,.085,silver,hip);rod('Angular shin',b,c,.095,.025,edge,knee)
  rod('Hooked foot',c,(c[0]-side*.14,c[1]+.12,.025),.055,0,dark,knee)
  for frame in range(1,18):
   t=(frame-1)/16*math.tau+(j%2)*math.pi+(0 if side==1 else math.pi)
   hip.rotation_euler=(.12*math.sin(t),side*.15*max(0,math.sin(t)),.18*math.cos(t));hip.keyframe_insert('rotation_euler',frame=frame)
   knee.rotation_euler=(0,side*.18*max(0,math.sin(t)),0);knee.keyframe_insert('rotation_euler',frame=frame)
 # front elbows and split pincers
 a=(side*.64,-.53,1.13);b=(side*1.04,-1.12,1.16);c=(side*.89,-1.68,1.14)
 claw=pivot(f'Claw {side}',a);rod('Armored claw arm',a,b,.18,.16,silver,claw);rod('Claw wrist',b,c,.16,.22,dark,claw)
 o=ellipsoid('Oversized pincer palm',c,(.33,.4,.23),silver);o.parent=claw;o.matrix_parent_inverse=claw.matrix_world.inverted()
 for sign in [-1,1]:
  start=(c[0]+sign*.19,c[1]-.17,c[2]);tip=(c[0]+sign*.19,c[1]-.68,c[2]+.02);end=(c[0]+sign*.025,c[1]-.86,c[2]+.02)
  rod('Pincer blade',start,tip,.14,.095,edge,claw);rod('Inward hook',tip,end,.095,0,edge,claw)
 for frame in range(1,18):
  claw.rotation_euler=(.035*math.sin((frame-1)/16*math.tau),0,side*.06*math.sin((frame-1)/16*math.tau));claw.keyframe_insert('rotation_euler',frame=frame)
 rod('Eye stalk',(side*.35,-.57,1.36),(side*.42,-.87,1.57),.065,.08,dark)
 ellipsoid('Amber inset lens',(side*.42,-.92,1.58),(.1,.07,.095),eye,12,6)
for j in range(4):
 rod('Mouth mandible',((j-1.5)*.13,-.72,1.01),((j-1.5)*.09,-1.02,.92),.075,.015,dark)
scene=bpy.context.scene;scene.frame_start=1;scene.frame_end=16;scene.render.fps=24
# Runtime coordinates: X right, Y up, +Z forward (Blender faces -Y).
frames=[];colors=[]
for frame in range(1,17):
 scene.frame_set(frame);deps=bpy.context.evaluated_depsgraph_get();vertices=[]
 for o in parts:
  ev=o.evaluated_get(deps);mesh=ev.to_mesh();mesh.calc_loop_triangles();mat=o.data.materials[0];col=tuple(mat.diffuse_color[:3])
  for tri in mesh.loop_triangles:
   for vi in tri.vertices:
    p=ev.matrix_world@mesh.vertices[vi].co;vertices.append((p.x,p.z,-p.y))
    if frame==1:colors.append(col)
  ev.to_mesh_clear()
 frames.append(vertices)
with (ASSET/'silver_spider_crab.bin').open('wb') as f:
 f.write(b'SSC1');f.write(struct.pack('<II',len(frames[0]),len(frames)))
 for c in colors:f.write(struct.pack('<3f',*c))
 for frame in frames:
  for p in frame:f.write(struct.pack('<3f',*p))
(OUT/'model.json').write_text(json.dumps({'name':'Silver spider crab','triangles':len(colors)//3,'frames':16,'size_metres':[4.4,1.8,3.8],'axes':'Blender Z up, faces -Y; runtime Y up, faces +Z','animations':'16-frame alternating eight-leg scuttle; two articulated claws','runtime_asset':'Procedures/Assets/Wildlife/silver_spider_crab.bin'},indent=2))
# Studio preview; all geometry remains individually editable.
scene.frame_set(4)
bpy.ops.object.camera_add(location=(6,-8,5));camera=bpy.context.object;camera.rotation_euler=(Vector((0,-.3,.8))-camera.location).to_track_quat('-Z','Y').to_euler();scene.camera=camera;camera.data.type='ORTHO';camera.data.ortho_scale=6.5
for loc,power,size in [((0,-4,7),1600,5),((5,2,4),2200,4),((-4,0,3),1300,3)]:
 bpy.ops.object.light_add(type='AREA',location=loc);o=bpy.context.object;o.data.energy=power;o.data.shape='DISK';o.data.size=size;o.rotation_euler=(-o.location).to_track_quat('-Z','Y').to_euler()
scene.world.color=(.15,.15,.15);scene.render.engine='CYCLES';scene.cycles.samples=32
scene.render.resolution_x=1100;scene.render.resolution_y=900;scene.render.resolution_percentage=100
scene.render.image_settings.file_format='PNG';scene.render.filepath=str(OUT/'SilverSpiderCrab.png')
bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'SilverSpiderCrab.blend'));bpy.ops.render.render(write_still=True)
print('SILVER_CRAB_EXPORTED',len(colors)//3)
