"""Compare packet BVH entry/exit queries with the authored triangle queries.

Source vertices and edge subtraction use their original GPU float32 rounding.
Ray arithmetic uses Python doubles: this checks geometry/query equivalence,
not GPU execution, performance or pixel-identical shader compilation.
"""
import math, random, runpy, struct
from pathlib import Path
S=runpy.run_path(str(Path(__file__).with_name('export_workstation_controls.py')))
f32=S['f32'];rows=[[f32(v) for v in row] for row in S['rows']]
dot=S['dot'];cross=S['cross'];rng=random.Random(3182)
prepared={}
def sub(a,b):return [x-y for x,y in zip(a,b)]
def unit(v):
 l=math.sqrt(dot(v,v));return [x/l for x in v]
def edges_hit(a,e1,e2,ns,flat,material,o,d,inside=True):
 h=cross(d,e2);det=dot(e1,h)
 if abs(det)<1e-12:return None
 off=sub(o,a);u=dot(off,h)/det;q=cross(off,e1);v=dot(d,q)/det;distance=dot(e2,q)/det
 if u<0 or v<0 or u+v>1 or distance<=1e-7:return None
 n=unit(ns[0] if flat else [ns[0][i]*(1-u-v)+ns[1][i]*u+ns[2][i]*v for i in range(3)])
 return (distance,n,material) if (dot(d,n)>0 if inside else dot(d,n)<0) else None
def tri_hit(t,o,d,inside=True):
 if id(t) not in prepared:
  vs,ns,material=t;vs=[[f32(v) for v in point] for point in vs];ns=[[f32(v) for v in n] for n in ns]
  edges=[[f32(point[i]-vs[0][i]) for i in range(3)] for point in vs[1:]]
  prepared[id(t)]=(vs,ns,material,edges)
 vs,ns,material,edges=prepared[id(t)]
 return edges_hit(vs[0],*edges,ns,False,material,o,d,inside)
def packet_hit(at,o,d,inside):
 a=rows[at][:3];ns=[r[:3] for r in rows[at+3:at+6]];flat=rows[at+3][3]>.5
 assert not flat or ns[0]==ns[1]==ns[2]
 if rows[at+1][3]>.5:edges=[rows[at+1][:3],rows[at+2][:3]]
 else:edges=[[f32(v-a[i]) for i,v in enumerate(rows[at+j][:3])] for j in (1,2)]
 return edges_hit(a,*edges,ns,flat,int(rows[at][3]),o,d,inside)
def tree_hit(desc,origin,direction,inside,ranked=False,any_hit=False):
 actual=None;node=int(desc[0])-S['BASE'];end=int(desc[1])-S['BASE']
 while node<end:
  lo,hi,meta=rows[node:node+3];count=int(meta[0]);near=0.;far=actual[0] if actual else 1e20
  for i in range(3):
   inv=(-1 if direction[i]<0 else 1)/max(abs(direction[i]),1e-8)
   a=(lo[i]-1e-7-origin[i])*inv;b=(hi[i]+1e-7-origin[i])*inv;near=max(near,min(a,b));far=min(far,max(a,b))
  if far<near:node=int(lo[3])-S['BASE'];continue
  if count==0:node+=3;continue
  for j in range(count):
   at=int(hi[3])-S['BASE']+j*6;h=packet_hit(at,origin,direction,inside)
   if h and ranked:h=(*h,int(rows[at+2][3]))
   if h and (actual is None or h[0]<actual[0] or (ranked and h[0]==actual[0] and h[3]<actual[3])):
    actual=h
    if any_hit:return actual
  node=int(lo[3])-S['BASE']
 return actual

def meter_hit(metadata,origin,direction,any_hit=False):
 descriptor=rows[metadata['band_acceleration']-S['BASE']]
 if any(0<abs(c)<1e-8 for c in direction):return tree_hit((metadata['root'],metadata['end']),origin,direction,False,any_hit=any_hit)
 actual=tree_hit(descriptor,origin,direction,False,ranked=True,any_hit=any_hit)
 if actual and any_hit:return actual[:3]
 inv=[(-1 if c<0 else 1)/max(abs(c),1e-8) for c in direction]
 for channel in range(int(descriptor[3])):
  at=int(descriptor[2])-S['BASE']+2*channel;lo,hi=rows[at:at+2]
  near=0.;far=actual[0] if actual else 1e20
  for i in range(3):
   a=(lo[i]-1e-7-origin[i])*inv[i];b=(hi[i]+1e-7-origin[i])*inv[i]
   near=max(near,min(a,b));far=min(far,max(a,b))
  if far<near:continue
  count=int(lo[3]);records=int(hi[3])-S['BASE'];entry_y=origin[1]+near*direction[1]
  guard=4e-7+4e-6*(abs(origin[1])+abs(near*direction[1]));first_y=entry_y+(-guard if direction[1]>=0 else guard)
  left=0;right=count
  for _ in range(8):
   if left>=right:break
   middle=(left+right)//2
   if rows[records+4*middle][1]<first_y:left=middle+1
   else:right=middle
  band=min(left,count-1);step=1 if direction[1]>=0 else -1
  for _ in range(128):
   if not 0<=band<count:break
   low,high=rows[records+4*band][:2];limit=min(far,actual[0] if actual else 1e20)
   last_y=origin[1]+limit*direction[1];end_guard=4e-7+4e-6*(abs(origin[1])+abs(limit*direction[1]))
   if (step>0 and low>last_y+end_guard) or (step<0 and high<last_y-end_guard):break
   band_lo=[lo[0],low,lo[2]];band_hi=[hi[0],high,hi[2]];entry=0.;exit=actual[0] if actual else 1e20
   for i in range(3):
    a=(band_lo[i]-1e-7-origin[i])*inv[i];b=(band_hi[i]+1e-7-origin[i])*inv[i]
    entry=max(entry,min(a,b));exit=min(exit,max(a,b))
   if exit>=entry:
    for group in range(1,4):
     for reference in rows[records+4*band+group]:
      rank=int(reference);h=packet_hit(rank-S['BASE'],origin,direction,False)
      if h and (actual is None or h[0]<actual[0] or (h[0]==actual[0] and rank<actual[3])):
       actual=(*h,rank)
       if any_hit:return actual[:3]
   band+=step
 return actual[:3] if actual else None

def exterior_hit(metadata,origin,direction):
 if metadata.get('band_acceleration'):return meter_hit(metadata,origin,direction)
 return tree_hit((metadata['root'],metadata['end']),origin,direction,False)
checks=0;outside_checks=0;max_error=0.;max_normal_error=0.;edge_components=0
def compare(expected,actual,label):
 global checks,max_error,max_normal_error
 assert (actual is None)==(expected is None),(label,expected,actual)
 if actual:
  error=abs(actual[0]-expected[0]);normal_error=max(abs(a-b) for a,b in zip(actual[1],expected[1]))
  max_error=max(max_error,error);max_normal_error=max(max_normal_error,normal_error)
  assert actual[2]==expected[2],(label,'material',expected,actual)
  assert error<2e-8 and normal_error<1e-4,(label,error,normal_error)
 checks+=1
for asset,metadata in zip(S['assets'],S['metadata']):
 # Check all exterior-packet edges against the source's original float32
 # subtraction, accounting for BVH reordering without reconstructing vertices.
 expected_edges=set()
 for vertices,_,material in asset['triangles']:
  vertices=[[f32(v) for v in p] for p in vertices]
  edges=[f32(point[i]-vertices[0][i]) for point in vertices[1:] for i in range(3)]
  expected_edges.add(struct.pack('<10f',*vertices[0],*edges,material))
 for triangle in range(metadata['triangles']):
  at=metadata['end']-S['BASE']+triangle*6
  assert rows[at+1][3]==1.
  key=struct.pack('<10f',*rows[at][:3],*rows[at+1][:3],*rows[at+2][:3],rows[at][3])
  assert key in expected_edges,(asset['name'],'edge encoding',triangle)
  edge_components+=6
 for sample in range(160):
  source=rng.choice(asset['triangles']);center=[sum(v[i] for v in source[0])/3 for i in range(3)];n=unit(source[1][0])
  origin=[center[i]+n[i]*.03 for i in range(3)];direction=unit([-n[i]+rng.uniform(-.3,.3) for i in range(3)])
  hits=[h for t in asset['triangles'] if (h:=tri_hit(t,origin,direction,False))];expected=min(hits,key=lambda h:h[0]) if hits else None
  actual=exterior_hit(metadata,origin,direction)
  compare(expected,actual,(asset['name'],'outside',sample));outside_checks+=1
 for material in sorted(set(t[2] for t in asset['triangles'])):
  if S['mats'][material][0][3]<.0001:continue
  ts=[t for t in asset['triangles'] if t[2]==material];desc=rows[material*40+2];plane_count=int(desc[2])
  for sample in range(160):
   source=rng.choice(ts);center=[sum(v[i] for v in source[0])/3 for i in range(3)];n=unit(source[1][0]);origin=[center[i]-n[i]*2e-6 for i in range(3)];direction=unit([n[i]+rng.uniform(-.3,.3) for i in range(3)])
   hits=[h for t in ts if (h:=tri_hit(t,origin,direction))];expected=min(hits,key=lambda h:h[0]) if hits else None
   actual=None
   if plane_count:
    candidates=[]
    for p in rows[material*40+6:material*40+6+plane_count]:
     denom=dot(p[:3],direction)
     if denom>=1e-7:candidates.append(((p[3]-dot(p[:3],origin))/denom,p[:3],material))
    if candidates:
     nearest=min(candidates,key=lambda h:h[0])
     if nearest[0]>1e-7:actual=nearest
   else:
    actual=tree_hit(desc,origin,direction,True)
   compare(expected,actual,(asset['name'],material,'inside',sample))

# Dense specialized queries cover every band and material seam, all six entry
# directions, origins inside bands, channel-to-channel seams, corners, grazing
# directions and the conservative full-tree fallback. The reference intersects
# every original triangle in the old BVH order, including coincident faces.
band_checks=0
for asset,metadata in zip(S['assets'],S['metadata']):
 if not metadata.get('band_acceleration'):continue
 original_order=sorted(asset['triangles'],key=lambda t:asset['addresses'][id(t)])
 def brute(origin,direction):
  hits=[h for t in original_order if (h:=tri_hit(t,origin,direction,False))]
  return min(hits,key=lambda h:h[0]) if hits else None
 rays=[]
 for key,bands in S['band_channels'](asset['triangles']):
  xmin,xmax,zmin,zmax=key;xc=(xmin+xmax)/2;zc=(zmin+zmax)/2
  for low,high,_ in bands:
   yc=(low+high)/2
   rays.extend([([xc,yc,zmax+.0001],[0,0,-1]),([xc,yc,zmin-.0001],[0,0,1]),
                ([xmin-.0001,yc,zc],[1,0,0]),([xmax+.0001,yc,zc],[-1,0,0]),
                ([xc,yc,zc],[0,1,0]),([xc,yc,zc],[0,-1,0])])
  boundaries=[bands[0][0]]+[band[1] for band in bands]
  for y in boundaries:
   for x in (xmin,xc,xmax):
    rays.extend([([x,y,zmax+.0001],[0,0,-1]),([x,y,zmin-.0001],[0,0,1])])
   for offset in (-2e-8,2e-8):
    rays.append(([xc,y+offset,zmax+.0001],unit([.03,offset*1000,-1])))
  for _ in range(240):
   target=[rng.uniform(xmin,xmax),rng.uniform(bands[0][0],bands[-1][1]),rng.uniform(zmin,zmax)]
   direction=unit([rng.uniform(-1,1) for _ in range(3)])
   distance=rng.choice([0.,1e-6,.002,.1,10.])
   rays.append(([target[i]-direction[i]*distance for i in range(3)],direction))
  for tiny in (-1e-9,0.,1e-9):rays.append(([xc,0.,zmax+.0001],unit([tiny,tiny,-1])))
 for sample,(origin,direction) in enumerate(rays):
  expected=brute(origin,direction);actual=meter_hit(metadata,origin,direction)
  compare(expected,actual,(asset['name'],'band query',sample))
  reference=tree_hit((metadata['root'],metadata['end']),origin,direction,False)
  compare(reference,actual,(asset['name'],'reference BVH',sample))
  assert (meter_hit(metadata,origin,direction,True) is not None)==(expected is not None)
  band_checks+=1
print(f'{checks} control comparisons passed ({outside_checks} exterior); '
      f'{band_checks} dense band queries and any-hit comparisons; '
      f'{edge_components} edge components match float32 subtraction exactly; '
      f'maximum distance difference {max_error:.3g} m; '
      f'maximum normal-component difference {max_normal_error:.3g}')
