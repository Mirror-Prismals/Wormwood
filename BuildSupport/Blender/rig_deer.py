"""Rig the author's existing low-poly whitetail derivative without changing either original."""
import bpy, math, json, struct, hashlib
from pathlib import Path
from mathutils import Vector, Matrix
ROOT=Path(__file__).resolve().parents[2];ART=ROOT/'Art/Wildlife/Deer';OUT=ROOT/'Procedures/Assets/Wildlife'
ART.mkdir(parents=True,exist_ok=True);OUT.mkdir(parents=True,exist_ok=True)
source=Path(bpy.data.filepath)
mesh=next(o for o in bpy.context.scene.objects if o.type=='MESH')
for o in list(bpy.context.scene.objects):
 if o!=mesh:bpy.data.objects.remove(o,do_unlink=True)
mesh.data=mesh.data.copy();mesh.data.transform(mesh.matrix_world);mesh.matrix_world=Matrix.Identity(4)
rigdata=bpy.data.armatures.new('Whitetail skeleton');rig=bpy.data.objects.new('Whitetail_Rig',rigdata);bpy.context.collection.objects.link(rig)
bpy.context.view_layer.objects.active=rig;rig.select_set(True);bpy.ops.object.mode_set(mode='EDIT')
bones=[('body',(0,0,1.12),None),('neck',(-.52,0,1.28),'body'),('head',(-.91,0,1.74),'neck'),('tail',(.78,0,1.28),'body')]
for part,x,z in [('front',-.52,1.15),('hind',.58,1.19)]:
 for side,y in [('L',.18),('R',-.18)]:
  bones.extend([(part+'.'+side,(x,y,z),'body'),(part+'_lower.'+side,(x+(.08 if part=='hind' else 0),y,.57),part+'.'+side)])
for side,y in [('L',.22),('R',-.22)]:bones.append(('ear.'+side,(-.81,y,2.04),'head'))
for name,p,parent in bones:
 b=rigdata.edit_bones.new(name);b.head=p;b.tail=Vector(p)+Vector((0,.10,0))
 if parent:b.parent=rigdata.edit_bones[parent]
bpy.ops.object.mode_set(mode='OBJECT');rig.show_in_front=True
vg={n:mesh.vertex_groups.new(name=n)for n,_,_ in bones}
# Smooth skin transitions through the shoulder/hip and knees. Distinct antler,
# eye and ear components follow the head rather than stretching across joints.
materials={v.index:[] for v in mesh.data.vertices}
for p in mesh.data.polygons:
 for i in p.vertices:materials[i].append(mesh.data.materials[p.material_index].name)
for v in mesh.data.vertices:
 x,y,z=v.co;side='L'if y>=0 else'R';names=materials[v.index]
 if x>.78 and z>1.1:w={'tail':1.}
 elif z>1.95 and abs(y)>.25 and x>-.98 and not any('Antler'in n for n in names):w={'ear.'+side:1.}
 elif (x<-.98 and z>1.52) or any('Antler'in n or'Eyes'in n or'Nose'in n for n in names):w={'head':1.}
 elif z>1.50 and x<-.60:
  along=((x+.52)*(-.39)+(z-1.28)*.46)/(.39*.39+.46*.46)
  h=max(0.,min(1.,(along-.70)/.28));h=h*h*(3-2*h)
  w={'head':h,'neck':1-h}
 elif z<1.17 and (x<-.34 or x>.34):
  part='front'if x<0 else'hind';upper=part+'.'+side;lower=part+'_lower.'+side
  body=max(0.,min(1.,(z-1.0)/.17));knee=max(0.,min(1.,(.68-z)/.22));w={'body':body,upper:(1-body)*(1-knee),lower:(1-body)*knee}
 else:
  neck=max(0.,min(1.,(-x-.39)/.34))*max(0.,min(1.,(z-1.06)/.32));w={'neck':neck,'body':1-neck}
 for n,weight in w.items():
  if weight>0:vg[n].add([v.index],weight,'REPLACE')
mod=mesh.modifiers.new('Whitetail skin','ARMATURE');mod.object=rig;mesh.parent=rig;mesh.name='Whitetail_Game_Skin'
rig.animation_data_create();scene=bpy.context.scene;scene.render.fps=24
clips=[('Idle',48,2.),('Alert',24,1.),('Run',24,.4)]
for clip,count,duration in clips:
 a=bpy.data.actions.new('Deer_'+clip);rig.animation_data.action=a
 for f in range(count+1):
  phase=math.tau*f/count
  for p in rig.pose.bones:p.rotation_mode='XYZ';p.rotation_euler=(0,0,0);p.location=(0,0,0)
  if clip=='Run':
   rig.pose.bones['body'].location.z=.07*(1-math.cos(phase*2))
   rig.pose.bones['body'].rotation_euler.y=.06*math.sin(phase)
   rig.pose.bones['neck'].rotation_euler.y=-.12;rig.pose.bones['head'].rotation_euler.y=.1
   rig.pose.bones['tail'].rotation_euler.y=-.55
   for name,offset in [('front.L',0),('front.R',.11),('hind.L',.51),('hind.R',.62)]:
    t=phase+math.tau*offset
    rig.pose.bones[name].rotation_euler.y=.65*math.sin(t)
    rig.pose.bones[name.split('.')[0]+'_lower.'+name.split('.')[1]].rotation_euler.y=(-1 if name.startswith('front')else 1)*.85*max(0,math.cos(t))
  elif clip=='Alert':
   rig.pose.bones['neck'].rotation_euler.y=.08
   rig.pose.bones['head'].rotation_euler.z=.08*math.sin(phase)
   rig.pose.bones['tail'].rotation_euler.y=-.18
  else:
   rig.pose.bones['neck'].rotation_euler.y=-.22-.05*math.sin(phase)
   rig.pose.bones['head'].rotation_euler.y=-.07
   rig.pose.bones['head'].rotation_euler.z=.08*math.sin(phase)
   rig.pose.bones['tail'].rotation_euler.y=.04*math.sin(phase*2)
  for side,sgn in [('L',1),('R',-1)]:rig.pose.bones['ear.'+side].rotation_euler.z=sgn*.12*math.sin(phase+sgn)
  for p in rig.pose.bones:p.keyframe_insert('rotation_euler',frame=f+1);p.keyframe_insert('location',frame=f+1)
 a.use_fake_user=True;a['seconds']=duration
 for slot in a.slots:
  bag=a.layers[0].strips[0].channelbag(slot)
  if bag:
   for fc in bag.fcurves:
    for k in fc.keyframe_points:k.interpolation='LINEAR'
mesh.data.calc_loop_triangles();faces=[]
for t in mesh.data.loop_triangles:
 mat=mesh.data.materials[t.material_index];color=list(mat.diffuse_color[:3]);faces.append([*t.vertices,*color])
frames=[];pose_frames=[]
# Export source skin influences and evaluated joints for runtime retarget/IK.
leg_names=['front.L','front.R','hind.L','hind.R']
contacts=[];lengths=[]
for n in leg_names:
 part,side=n.split('.');sign=1 if side=='L' else -1
 candidates=[v.co for v in mesh.data.vertices if v.co.z<.16 and v.co.y*sign>0 and (v.co.x<-.34 if part=='front' else v.co.x>.34)]
 contact=Vector((sum(v.x for v in candidates)/len(candidates),sum(v.y for v in candidates)/len(candidates),min(v.z for v in candidates)))
 contacts.append(contact)
 upper=rigdata.bones[n].head_local;lower=rigdata.bones[part+'_lower.'+side].head_local
 lengths.append([(lower-upper).length,(contact-lower).length])
weights=[];hoof_weights=[]
for v in mesh.data.vertices:
 weights.append([next((g.weight for g in v.groups if mesh.vertex_groups[g.group].name==n),0.)for n,_,_ in bones])
 hoof_weights.append(max(0.,min(1.,(.24-v.co.z)/.10)))

for clip,count,duration in clips:
 rig.animation_data.action=bpy.data.actions['Deer_'+clip]
 for f in range(1,count+1):
  scene.frame_set(f);ev=mesh.evaluated_get(bpy.context.evaluated_depsgraph_get());data=ev.to_mesh()
  floor=min(v.co.z for v in data.vertices)
  # Grounded hoof envelope: the presentation root never drives a hoof through
  # the sampled surface. A small run suspension adds an authored aerial phase.
  lift=.04*max(0,math.sin(math.tau*(f-1)/count*2)) if clip=='Run' else 0
  cv=lambda v:[v.y,v.z-floor+lift,-v.x]
  direction=lambda v:[v.y,v.z,-v.x]
  legs=[];foot_up=[]
  for n,contact in zip(leg_names,contacts):
   part,side=n.split('.');upper=rig.pose.bones[n];lower=rig.pose.bones[part+'_lower.'+side]
   delta=lower.matrix @ lower.bone.matrix_local.inverted()
   legs.append([cv(upper.head),cv(lower.head),cv(delta @ contact)])
   foot_up.append(direction(delta.to_3x3() @ Vector((0,0,1))))
  head=rig.pose.bones['head'];head_delta=head.matrix @ head.bone.matrix_local.inverted()
  pose_frames.append({'legs':legs,'foot_up':foot_up,'neck':cv(rig.pose.bones['neck'].head),'head':cv(head.head),'head_forward':direction(head_delta.to_3x3() @ Vector((-1,0,0)))})
  frames.append([(v.co.y,v.co.z-floor+lift,-v.co.x)for v in data.vertices]);ev.to_mesh_clear()
with(OUT/'deer.bin').open('wb')as out:
 out.write(b'DEERSKIN');out.write(struct.pack('<III',len(mesh.data.vertices),len(faces),len(frames)))
 for face in faces:out.write(struct.pack('<IIIfff',*face))
 for frame in frames:
  for v in frame:out.write(struct.pack('<fff',*v))
(OUT/'deer_rig.json').write_text(json.dumps({'schema_version':1,'rigid_head_vertices':[v.index for v in mesh.data.vertices if (v.co.x<-.98 and v.co.z>1.52) or any('Antler'in n or'Eyes'in n or'Nose'in n for n in materials[v.index])],'bones':[n for n,_,_ in bones],'weights':weights,'hoof_weights':hoof_weights,'lengths':lengths,'frames':pose_frames},separators=(',',':'))+'\n')
report={'source':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'triangles':len(faces),'vertices':len(mesh.data.vertices),'bones':[n for n,_,_ in bones],'clips':[{'name':n,'frames':c,'seconds':s}for n,c,s in clips],'coordinates':'metres; +Y up, +Z forward; grounded hoof envelope'}
(ART/'export.json').write_text(json.dumps(report,indent=2)+'\n')
rig.animation_data.action=bpy.data.actions['Deer_Idle'];scene.frame_set(1);scene.frame_start=1;scene.frame_end=48
bpy.ops.wm.save_as_mainfile(filepath=str(ART/'Whitetail_Rigged.blend'));print('DEER_EXPORT',json.dumps(report))
