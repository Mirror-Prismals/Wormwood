"""Derive an animated game cardinal from the author's original; never overwrite it.
Run Blender --background Northern_Cardinal_HighPoly.blend --python this_file.
"""
import bpy, bmesh, math, json, struct, hashlib
from pathlib import Path
from mathutils import Vector, Matrix
ROOT = Path(__file__).resolve().parents[2]
ART = ROOT/'Art/Birds/Cardinal'
ASSET = ROOT/'Procedures/Assets/Birds'
ART.mkdir(parents=True, exist_ok=True); ASSET.mkdir(parents=True, exist_ok=True)
source = Path(bpy.data.filepath)
# The dense individual contour feathers are retained in the source. The game
# mesh keeps the actual continuous anatomy, flight feathers, crest and face.
keep=[]
for o in list(bpy.context.scene.objects):
    n=o.name
    if o.type!='MESH' or o.hide_render or n.startswith(('DISPLAY','Fine continuous','Breast and back','Head fine','Scapular','Leg scute','Toe scute','Wing rachis','Tail rachis')):
        bpy.data.objects.remove(o,do_unlink=True); continue
    bpy.context.view_layer.objects.active=o
    o.select_set(True)
    # Bake original transforms before weighting in source coordinates.
    o.data=o.data.copy()
    o.data.transform(o.matrix_world); o.matrix_world=Matrix.Identity(4)
    for mod in list(o.modifiers):
        if mod.type in {'SUBSURF','MULTIRES'}: o.modifiers.remove(mod)
        else:
            try: bpy.ops.object.modifier_apply(modifier=mod.name)
            except RuntimeError: o.modifiers.remove(mod)
    o.data.calc_loop_triangles()
    budget=(1200 if n.startswith('CARDINAL') else 260 if n.startswith('Wing | primary') else 120 if n.startswith('Wing under') else 130 if n.startswith('Tail |') else 90 if n.startswith('Wing | fitted') else 60 if n.startswith('Crest') else 60 if n.startswith('Bill |') else 40 if n.startswith('Eye') else 24)
    original_mesh=o.data.copy()
    bounds=[(min(v.co[i] for v in o.data.vertices),max(v.co[i] for v in o.data.vertices)) for i in range(3)]
    dec=o.modifiers.new('Game silhouette budget','DECIMATE'); dec.ratio=min(1.,budget/max(1,len(o.data.loop_triangles))); dec.use_collapse_triangulate=True
    bpy.ops.object.modifier_apply(modifier=dec.name)
    # Thin bill shells can produce outlying collapse vertices; preserve their
    # authored geometry whenever simplification leaves the original bounds.
    if n.startswith('Bill |') and any(v.co[i]<bounds[i][0]-.0002 or v.co[i]>bounds[i][1]+.0002 for v in o.data.vertices for i in range(3)):
        o.data=original_mesh
    else:
        bpy.data.meshes.remove(original_mesh)
    keep.append(o);o.select_set(False)
# Source axes: forward -X, side Y, up Z. Bones use matching local axes,
# allowing deterministic animation baking and editable Blender actions.
arm=bpy.data.armatures.new('Cardinal skeleton');rig=bpy.data.objects.new('Cardinal_Rig',arm);bpy.context.collection.objects.link(rig)
bpy.context.view_layer.objects.active=rig;rig.select_set(True);bpy.ops.object.mode_set(mode='EDIT')
bones=[('body',(0,0,.11),None),('head',(-.027,0,.157),'body'),('jaw',(-.060,0,.174),'head'),('wing.L',(-.018,.024,.151),'body'),('wing.R',(-.018,-.024,.151),'body'),('tail',(.034,0,.09),'body'),('leg.L',(.008,.016,.069),'body'),('leg.R',(.008,-.016,.069),'body')]
for name,p,parent in bones:
    b=arm.edit_bones.new(name);b.head=p;b.tail=Vector(p)+Vector((0,.012,0))
    if parent:b.parent=arm.edit_bones[parent]
bpy.ops.object.mode_set(mode='OBJECT');rig.show_in_front=True
for o in keep:
    groups={name:o.vertex_groups.new(name=name) for name,_,_ in bones}
    n=o.name
    for v in o.data.vertices:
        x,y,z=v.co;side='L' if y>=0 else 'R'
        if n.startswith('Wing'): weights={'wing.'+side:1.}
        elif n.startswith('Tail'):weights={'tail':1.}
        elif n.startswith(('Leg','Foot','Toe','Hallux')):weights={'leg.'+side:1.}
        elif n.startswith('Bill | lower'):weights={'jaw':1.}
        elif n.startswith(('Eye','Bill','Crest')):weights={'head':1.}
        else:
            h=max(0.,min(1.,(z-.143)/.028));weights={'head':h,'body':1-h}
        for name,w in weights.items():
            if w>0:groups[name].add([v.index],w,'REPLACE')
    mod=o.modifiers.new('Cardinal skin','ARMATURE');mod.object=rig;o.parent=rig
# Join skin into a single draw mesh, preserving material slots and weights.
bpy.ops.object.select_all(action='DESELECT')
for o in keep:o.select_set(True)
bpy.context.view_layer.objects.active=keep[0];bpy.ops.object.join();mesh=bpy.context.object;mesh.name='Cardinal_Game_Skin'
bm=bmesh.new();bm.from_mesh(mesh.data)
bmesh.ops.delete(bm,geom=[v for v in bm.verts if not v.link_faces],context='VERTS')
bm.to_mesh(mesh.data);bm.free()
# Explicit triangulation makes every baked frame share identical topology.
tri=mesh.modifiers.new('Stable export triangles','TRIANGULATE');bpy.ops.object.modifier_move_up(modifier=tri.name);bpy.ops.object.modifier_apply(modifier=tri.name)
rig.animation_data_create();scene=bpy.context.scene;scene.render.fps=24
clips=[('Perch',48,2.),('Flight',16,.16),('Call',24,1.)]
# A folded wing sweeps laterally around the shoulder before flapping. Body and
# head counter-motion, tucked legs and tail pitch accompany flight.
for clip,count,duration in clips:
    action=bpy.data.actions.new('Cardinal_'+clip);rig.animation_data.action=action
    for frame in range(count+1):
        u=frame/count;phase=2*math.pi*u
        for p in rig.pose.bones:p.rotation_mode='XYZ';p.rotation_euler=(0,0,0);p.location=(0,0,0)
        if clip=='Flight':
            for side,sgn in [('L',1),('R',-1)]:
                rig.pose.bones['wing.'+side].rotation_euler=(sgn*(.10+.85*math.sin(phase)),0,sgn*1.35)
                rig.pose.bones['leg.'+side].rotation_euler=(0,-.85,0)
            rig.pose.bones['body'].rotation_euler.y=-.45+.05*math.sin(phase)
            rig.pose.bones['head'].rotation_euler.y=.25
            rig.pose.bones['tail'].rotation_euler.y=.12+.12*math.sin(phase+.7)
        elif clip=='Perch':
            rig.pose.bones['head'].rotation_euler=(.08*math.sin(phase),.045*math.sin(phase*2),.24*math.sin(phase))
            rig.pose.bones['tail'].rotation_euler.y=.08*math.sin(phase*2)
            rig.pose.bones['body'].location.z=.001*math.sin(phase*2)
        else:
            rig.pose.bones['jaw'].rotation_euler.y=-.32*max(0,math.sin(phase*3))
            rig.pose.bones['head'].rotation_euler.y=.10+.04*math.sin(phase*3)
            rig.pose.bones['tail'].rotation_euler.y=.06*math.sin(phase*2)
        for p in rig.pose.bones:
            p.keyframe_insert('rotation_euler',frame=frame+1);p.keyframe_insert('location',frame=frame+1)
    action.use_fake_user=True
    # Force linear interpolation for seamless runtime sample interpolation.
    for slot in action.slots:
        bag=action.layers[0].strips[0].channelbag(slot)
        if bag:
            for fc in bag.fcurves:
                for k in fc.keyframe_points:k.interpolation='LINEAR'
    action['seconds']=duration
rig.animation_data.action=bpy.data.actions['Cardinal_Perch'];scene.frame_set(1)
# Export unique vertices and indexed colored triangles plus evaluated skin frames.
mesh.data.calc_loop_triangles();faces=[]
for t in mesh.data.loop_triangles:
    mat=mesh.data.materials[t.material_index];color=list(mat.diffuse_color[:3])
    if mat.use_nodes:
        bs=next((n for n in mat.node_tree.nodes if n.type=='BSDF_PRINCIPLED'),None)
        if bs:color=list(bs.inputs['Base Color'].default_value[:3])
    faces.append([*t.vertices,*color])
frames=[]
for clip,count,duration in clips:
    rig.animation_data.action=bpy.data.actions['Cardinal_'+clip]
    for f in range(1,count+1):
        scene.frame_set(f);dg=bpy.context.evaluated_depsgraph_get();ev=mesh.evaluated_get(dg);data=ev.to_mesh()
        # World metres, y up, +z forward; feet meet the existing branch anchor.
        frames.append([(v.co.y,v.co.z-.032,-v.co.x) for v in data.vertices]);ev.to_mesh_clear()
with (ASSET/'cardinal.bin').open('wb') as out:
    out.write(b'CARDINAL');out.write(struct.pack('<III',len(mesh.data.vertices),len(faces),len(frames)))
    for face in faces:out.write(struct.pack('<IIIfff',*face))
    for frame in frames:
        for v in frame:out.write(struct.pack('<fff',*v))
report={'source':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'vertices':len(mesh.data.vertices),'triangles':len(faces),'bones':[n for n,_,_ in bones],'clips':[{'name':n,'frames':c,'seconds':d} for n,c,d in clips],'coordinate_system':'metres; +X right, +Y up, +Z forward; foot branch anchor','omitted':'display branch and dense microplumage; original file preserved'}
(ART/'export.json').write_text(json.dumps(report,indent=2)+'\n')
rig.animation_data.action=bpy.data.actions['Cardinal_Perch'];scene.frame_set(1);scene.frame_start=1;scene.frame_end=48
bpy.ops.wm.save_as_mainfile(filepath=str(ART/'Cardinal_Rigged.blend'))
print('CARDINAL_EXPORT',json.dumps(report))
