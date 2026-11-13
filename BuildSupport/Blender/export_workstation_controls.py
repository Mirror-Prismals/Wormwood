"""Pack the authored static GLBs into triangle BVHs; no geometry simplification.
Node transforms, component materials, normals and exported meter levels are kept.
Run from any directory. Runtime only instances this immutable decorative kit.
"""
import argparse, hashlib, json, math, struct, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser()
parser.add_argument('--source-root',type=Path,default=ROOT)
parser.add_argument('--output',type=Path)
parser.add_argument('--meter-bands',action='store_true')
args=parser.parse_args()
ART = args.source_root / 'Art/AudioWorkstation'
BASE = 256 + 3*64*64
CAPACITY = 32768
FILES = ['GemButtons/GemButton_'+n+'.glb' for n in ['Rewind','Stop','Record','Play']] + ['GemFadersMeters/GemFader_Ivory.glb','GemFadersMeters/GemMeter_Ivory.glb']
engraving=json.loads((ART/'ControlEngraving/control_engraving.json').read_text())
assert engraving['schema']==1
engraved_assets={asset['name']:asset for asset in engraving['assets']}
patterns={pattern['id']:pattern for pattern in engraving['patterns']}
FILES=['ControlEngraving/'+engraved_assets[Path(file).stem]['glb']
       if Path(file).stem in engraved_assets else file for file in FILES]
# The exact meter-band experiment preserved pixels but regressed GPU time on
# the measured M1 Pro. Keep it opt-in for diagnostics, out of production data.
METER_BANDS=args.meter_bands

def mul(a,b): return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
ID = [[float(i==j) for j in range(4)] for i in range(4)]
def transform(n):
    if 'matrix' in n: return [[n['matrix'][j*4+i] for j in range(4)] for i in range(4)]
    x,y,z,w=n.get('rotation',[0,0,0,1]); s=n.get('scale',[1,1,1]);t=n.get('translation',[0,0,0])
    a=[[1-2*(y*y+z*z),2*(x*y-z*w),2*(x*z+y*w),t[0]], [2*(x*y+z*w),1-2*(x*x+z*z),2*(y*z-x*w),t[1]], [2*(x*z-y*w),2*(y*z+x*w),1-2*(x*x+y*y),t[2]], [0,0,0,1]]
    for i in range(3):
        for j in range(3): a[i][j]*=s[j]
    return a

def vec(m,v,normal=False):
    # Supplied assets use rigid node transforms; reject unexpected deformation.
    if normal:
        for i in range(3):
            for j in range(3): assert abs(sum(m[k][i]*m[k][j] for k in range(3))-(i==j))<1e-5
    v=[sum(m[i][j]*v[j] for j in range(3))+(0 if normal else m[i][3]) for i in range(3)]
    return [v[0],-v[2],v[1]] # glTF Y-up -> authored Z-out

rows=[]; assets=[]; sources={}; mats=[]
runtime=json.loads((ART/'GemPanels/GemPanels_runtime.json').read_text())
energy=runtime['materials'][0]['ggx_multiscatter']
for file in FILES:
    raw=(ART/file).read_bytes();sources[file]=hashlib.sha256(raw).hexdigest()
    size=struct.unpack_from('<I',raw,12)[0]; g=json.loads(raw[20:20+size]);binary=raw[28+size:]
    def accessor(index):
        a=g['accessors'][index];v=g['bufferViews'][a['bufferView']];assert not a.get('sparse')
        kind={5126:'f',5125:'I',5123:'H',5121:'B'}[a['componentType']]; count={'VEC3':3,'SCALAR':1}[a['type']]
        fmt='<'+kind*count; stride=v.get('byteStride',struct.calcsize(fmt));offset=v.get('byteOffset',0)+a.get('byteOffset',0)
        return [struct.unpack_from(fmt,binary,offset+i*stride) for i in range(a['count'])]
    material_offset=len(mats)
    for m in g['materials']:
        p=m.get('pbrMetallicRoughness',{});e=m.get('extensions',{});volume=e.get('KHR_materials_volume',{})
        sigma=[-math.log(max(x,1e-20))/volume.get('attenuationDistance',1) for x in volume.get('attenuationColor',[1,1,1])]
        mat=[[0.,0.,0.,0.] for _ in range(40)]
        ior=e.get('KHR_materials_ior',{}).get('ior',1.5)
        mat[0]=[1,ior,p.get('roughnessFactor',1),e.get('KHR_materials_transmission',{}).get('transmissionFactor',0)]
        mat[1]=sigma+[0];mat[3]=[.001,.001,.001,0];mat[4]=p.get('baseColorFactor',[1,1,1,1]);mat[5]=m.get('emissiveFactor',[0,0,0])+[0]
        for start,key in [(32,'ggx_glass_E'),(36,'ggx_glass_inv_E')]:
            values=energy[key] if abs(ior-1.766)<1e-4 else [1.]*16
            for j in range(4): mat[start+j]=values[j*4:j*4+4]
        mats.append(mat)
    tris=[]; moving=[]; static=[]; spectrum=[]; moving_nodes=[]
    def visit(index,parent,slider=False):
        node=g['nodes'][index];m=mul(parent,transform(node));assert 'skin' not in node
        slider=slider or node.get('name','').endswith('_Slider')
        if slider:moving_nodes.append(node.get('name',''))
        if 'mesh' in node:
            for p in g['meshes'][node['mesh']]['primitives']:
                assert p.get('mode',4)==4 and 'targets' not in p
                pos=[vec(m,v) for v in accessor(p['attributes']['POSITION'])]
                normals=[vec(m,v,True) for v in accessor(p['attributes']['NORMAL'])]
                ids=[i[0] for i in accessor(p['indices'])]
                for i in range(0,len(ids),3):
                    ix=ids[i:i+3];triangle=([pos[j] for j in ix],[normals[j] for j in ix],material_offset+p['material']);tris.append(triangle)
                    (moving if slider else static).append(triangle)
                channel=node.get('extras',{}).get('channel')
                if channel in ('L','R'):
                    spectrum.append({'channel':channel,'material':material_offset+p['material'],'name':g['materials'][p['material']]['name']})
        for child in node.get('children',[]): visit(child,m,slider)
    for root in g['scenes'][g.get('scene',0)]['nodes']: visit(root,ID)
    name=Path(file).stem
    asset={'name':name,'triangles':tris}
    if name=='GemFader_Ivory':
        authored=next(n for n in g['nodes'] if n.get('name')==name)['extras']
        low,high=authored['slider_travel_y_m'];preview=authored['preview_value']
        assert moving and static and len(moving)+len(static)==len(tris)
        assert set(moving_nodes)=={name+'_Slider',name+'_Cap',name+'_Stem',name+'_Index'}
        assert 0<=preview<=1 and low<high
        asset['motion']={'stationary':static,'slider':moving,'travel_y_m':[low,high],
                         'preview_y_m':low+preview*(high-low),'preview_value':preview,'source_nodes':moving_nodes}
    if name=='GemMeter_Ivory':asset['spectrum']=spectrum
    if name in engraved_assets:
        entry=engraved_assets[name]
        assert hashlib.sha256(raw).hexdigest()==entry['glb_sha256'], (name,'engraved GLB changed')
        host_material=int(entry['host_material_index'])
        assert g['materials'][host_material]['name']==entry['host_material']
        assert mats[material_offset+host_material][0][3]>0.0
        host=next(node for node in g['nodes'] if node.get('name')==entry['host_node'])
        assert all(p['material']==host_material for p in g['meshes'][host['mesh']]['primitives'])
        assert not any(node.get('name') in entry['removed_nodes'] for node in g['nodes'])
        assert not entry['native_glb_has_fracture_triangles']
        asset['engraving']=(material_offset+host_material,entry,patterns[entry['pattern_id']])
    assets.append(asset)
assert len(mats)<256
for m in mats: rows.extend(m)
metadata=[]
interior_checks=[]
def cross(a,b): return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
def dot(a,b): return sum(x*y for x,y in zip(a,b))
def f32(v): return struct.unpack('<f',struct.pack('<f',v))[0]
def convex_planes(ts):
    planes=[]
    for vertices,normals,_ in ts:
        e1=[vertices[1][i]-vertices[0][i] for i in range(3)]
        e2=[vertices[2][i]-vertices[0][i] for i in range(3)]
        n=cross(e1,e2);length=math.sqrt(dot(n,n))
        if length<1e-15: return []
        n=[v/length for v in n];d=dot(n,vertices[0])
        # Analytic exits may only replace flat facets with identical normals.
        if any(max(abs(n[i]-v[i]) for i in range(3))>1e-5 for v in normals): return []
        if not any(max(abs(n[i]-p[i]) for i in range(3))<1e-6 and abs(d-p[3])<1e-8 for p in planes):
            planes.append(n+[d])
    if len(planes)>26: return []
    if any(dot(p[:3],v)-p[3]>2e-8 for p in planes for t in ts for v in t[0]): return []
    return planes

def pack_tree(tris, source_ranks=None):
    nodes=[];ordered=[]
    def build(ts):
        idx=len(nodes);nodes.append(None)
        lo=[min(t[0][j][k] for t in ts for j in range(3)) for k in range(3)]
        hi=[max(t[0][j][k] for t in ts for j in range(3)) for k in range(3)]
        first=len(ordered);count=0
        if len(ts)<=4: ordered.extend(ts);count=len(ts)
        else:
            # Surface-area splits avoid repeatedly visiting overlapping long
            # housing triangles when tracing the narrow meter and fader models.
            def area(a,b):
                extent=[max(0.,b[i]-a[i]) for i in range(3)]
                return 2*(extent[0]*extent[1]+extent[1]*extent[2]+extent[2]*extent[0])
            best=None
            centroids=[[sum(v[i] for v in t[0])/3 for i in range(3)] for t in ts]
            for axis in range(3):
                lower=min(c[axis] for c in centroids);upper=max(c[axis] for c in centroids)
                if upper-lower<1e-12:continue
                for bucket in range(1,12):
                    cut=lower+(upper-lower)*bucket/12
                    groups=([t for t,c in zip(ts,centroids) if c[axis]<cut],
                            [t for t,c in zip(ts,centroids) if c[axis]>=cut])
                    if min(map(len,groups))<2:continue
                    cost=0.
                    for group in groups:
                        a=[min(v[i] for t in group for v in t[0]) for i in range(3)]
                        b=[max(v[i] for t in group for v in t[0]) for i in range(3)]
                        cost+=area(a,b)*len(group)
                    if best is None or cost<best[0]:best=(cost,groups)
            if best is None:
                axis=max(range(3),key=lambda i:hi[i]-lo[i]);ts=sorted(ts,key=lambda t:sum(v[axis] for v in t[0]))
                split=len(ts)//2;groups=(ts[:split],ts[split:])
            else:groups=best[1]
            build(groups[0]);build(groups[1])
        nodes[idx]=(lo,hi,len(nodes),first,count,min(t[2] for t in ts),max(t[2] for t in ts))
    build(tris)
    start=BASE+len(rows);tri_start=start+3*len(nodes)
    for lo,hi,end,first,count,material_min,material_max in nodes:
        rows.extend([lo+[start+3*end],hi+[tri_start+6*first],[count,material_min,material_max,0]])
    addresses={}
    for triangle in ordered:
        positions,normals,material=triangle
        addresses[id(triangle)]=BASE+len(rows)
        # Match GPU float32 subtraction of the previously packed positions.
        # Six rows and every tree pointer stay unchanged. A flagged edge row
        # avoids per-ray edge subtraction; a flagged normal avoids fetching
        # and interpolating three copies of the identical authored flat normal.
        positions=[[f32(v) for v in point] for point in positions]
        normals=[[f32(v) for v in normal] for normal in normals]
        edges=[[f32(point[i]-positions[0][i]) for i in range(3)] for point in positions[1:]]
        flat=normals[0]==normals[1]==normals[2]
        rank=source_ranks[id(triangle)] if source_ranks is not None else 0
        rows.extend([positions[0]+[material],edges[0]+[1],edges[1]+[rank],
                     normals[0]+[int(flat)],normals[1]+[0],normals[2]+[0]])
    return start,tri_start,nodes[0][0],nodes[0][1],addresses

def band_channels(triangles):
    """Recognize exact exported boxes, never replace or fit their surfaces.

    Only an opaque twelve-triangle box with two exact float32 coordinates per
    axis qualifies. Each face must contain the two original triangles covering
    its four corners, with its authored outward axis normal. Contiguous boxes
    with identical X/Z extents form an ordered channel. Any unrecognized shape
    stays in the ordinary BVH.
    """
    materials={}
    for triangle in triangles: materials.setdefault(triangle[2],[]).append(triangle)
    channels={}
    for material,ts in materials.items():
        if len(ts)!=12 or mats[material][0][3]>=.0001: continue
        positions=[[[f32(c) for c in v] for v in t[0]] for t in ts]
        axes=[sorted({v[i] for t in positions for v in t}) for i in range(3)]
        if any(len(a)!=2 or a[0]==a[1] for a in axes): continue
        faces={};valid=True
        for triangle,vertices in zip(ts,positions):
            n=triangle[1][0]
            if any(normal!=n for normal in triangle[1]): valid=False;break
            axis=next((i for i in range(3) if abs(n[i])==1 and all(n[j]==0 for j in range(3) if j!=i)),None)
            if axis is None:valid=False;break
            side=int(n[axis]>0)
            if any(v[axis]!=axes[axis][side] for v in vertices):valid=False;break
            faces.setdefault((axis,side),[]).append({tuple(v) for v in vertices})
        if not valid or len(faces)!=6:continue
        for (axis,side),face in faces.items():
            if len(face)!=2 or any(len(t)!=3 for t in face):valid=False;break
            shared=face[0]&face[1]
            if len(face[0]|face[1])!=4 or len(shared)!=2:valid=False;break
            a,b=shared
            if any(a[i]==b[i] for i in range(3) if i!=axis):valid=False;break
        if valid:
            key=(axes[0][0],axes[0][1],axes[2][0],axes[2][1])
            channels.setdefault(key,[]).append((axes[1][0],axes[1][1],ts))
    result=[]
    for key,bands in sorted(channels.items()):
        bands.sort(key=lambda band:band[0])
        if not 2<=len(bands)<=128:continue
        if any(a[1]!=b[0] for a,b in zip(bands,bands[1:])):continue
        result.append((key,bands))
    return result if len(result)==2 else []

def pack_bands(asset,metadata):
    channels=band_channels(asset['triangles'])
    if not channels:return
    selected={id(t) for _,bands in channels for _,_,ts in bands for t in ts}
    remainder=[t for t in asset['triangles'] if id(t) not in selected]
    start,end,_,_,_=pack_tree(remainder,asset['addresses'])
    descriptor=BASE+len(rows);rows.append([start,end,descriptor+1,len(channels)])
    header=len(rows);rows.extend([[0.,0.,0.,0.] for _ in range(2*len(channels))])
    for channel,(key,bands) in enumerate(channels):
        xmin,xmax,zmin,zmax=key;records=BASE+len(rows)
        rows[header+2*channel]=[xmin,bands[0][0],zmin,len(bands)]
        rows[header+2*channel+1]=[xmax,bands[-1][1],zmax,records]
        for low,high,ts in bands:
            addresses=sorted(asset['addresses'][id(t)] for t in ts)
            rows.append([low,high,0,0])
            rows.extend(addresses[i:i+4] for i in range(0,12,4))
    metadata['band_acceleration']=descriptor
    metadata['band_triangles']=len(selected)
    metadata['reduced_triangles']=len(remainder)

for asset in assets:
    start,end,lo,hi,addresses=pack_tree(asset['triangles'])
    asset['addresses']=addresses
    metadata.append({'name':asset['name'],'root':start,'end':end,'min':lo,'max':hi,'triangles':len(asset['triangles'])})
    for material in sorted(set(t[2] for t in asset['triangles'])):
        if mats[material][0][3]<0.0001: continue
        triangles=[t for t in asset['triangles'] if t[2]==material]
        planes=convex_planes(triangles)
        if planes:
            rows[material*40+2]=[0,0,len(planes),0]
            for i,plane in enumerate(planes): rows[material*40+6+i]=plane
        else:
            interior_start,interior_end,_,_,_=pack_tree(triangles)
            rows[material*40+2]=[interior_start,interior_end,0,0]
        interior_checks.append({'asset':asset['name'],'material':material,'planes':len(planes),'triangles':len(triangles)})
# Append acceleration after the complete reference packet. Existing BVHs and
# triangle bytes remain available as the fallback and stable tie-order source.
for asset,model in zip(assets,metadata):
    if METER_BANDS and asset['name']=='GemMeter_Ivory':pack_bands(asset,model)
    if 'engraving' in asset:
        material,entry,pattern=asset['engraving']
        address=BASE+len(rows)
        mips=pattern['density_mips']
        rows.extend([pattern['origin_m']+pattern['extent_m'],
                     entry['layer_z_m']+[pattern['unit_m']],
                     [pattern['density_offset_rows'],len(mips),address+3,0]])
        rows.extend(mip['size_px']+[mip['offset_texels'],0] for mip in mips)
        rows[material*40+2][3]=address
        model['engraving_material']=material
        model['engraving_descriptor']=address
        model['engraving_pattern']=entry['pattern_id']
# Append derived component trees after the complete reference packet. Their
# materials, vertices and normals remain the original GLB values, and every
# preexisting triangle/tree/SSLE address is unchanged.
component_prefix_rows=len(rows)
for asset,model in zip(assets,metadata):
    if 'motion' in asset:
        motion=asset['motion'];components={}
        for kind in ('stationary','slider'):
            ts=motion[kind];start,end,lo,hi,_=pack_tree(ts)
            components[kind]={'root':start,'end':end,'min':lo,'max':hi,'triangles':len(ts)}
        model['motion_components']={**{k:v for k,v in motion.items() if k not in ('stationary','slider')},**components}
    if 'spectrum' in asset:
        model['live_spectrum']={'local_y_m':[-.078,.078],'range_db':[-60,0],'steps':60,
           'inactive_multiplier':.25,'emission_strength':.85,
           'palette':[[0,0,1],[0,1,1],[0,1,0],[1,1,0],[1,166/255,0],[1,0,0]],
           'materials':asset['spectrum']}
# Reserve existing 260-row instance hierarchy; component metadata may not
# silently overwrite it. No optimization or existing geometry is replaced.
assert BASE+len(rows)<=40992,(BASE+len(rows),40992)
assert all(math.isfinite(v) for r in rows for v in r)
out=args.output or ART/'DecorativeControls';out.mkdir(parents=True,exist_ok=True)
(out/'controls.bin').write_bytes(b''.join(struct.pack('<4f',*r) for r in rows))
(out/'controls.json').write_text(json.dumps({'schema':2,'triangle_encoding':'flagged_float32_edges_and_flat_normals','base':BASE,'capacity':CAPACITY,'rows':len(rows),'component_prefix_rows':component_prefix_rows,'dynamic_reserved_ranges':{'editor':[40992,43152],'mixer':[43152,43664],'track_io':[44288,44592],'instance_bvh':[44812,45312]},'materials':len(mats),'assets':metadata,'source_sha256':sources,'interior_acceleration':interior_checks,'meter_band_acceleration_enabled':METER_BANDS,'control_engraving_manifest_sha256':hashlib.sha256((ART/'ControlEngraving/control_engraving.json').read_bytes()).hexdigest()},indent=2)+'\n')
print(json.dumps(interior_checks))
print(f'Packed {sum(a["triangles"] for a in metadata)} authored triangles, {len(mats)} materials, {len(rows)} rows')
