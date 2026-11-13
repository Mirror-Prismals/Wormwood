#!/usr/bin/env python3
"""Private compiler for a single fixed-view, flat-faceted ruby control.

python3 BuildSupport/Workstation/compiled_gem_paths.py --frame isolated.bin
  --output output/checks/ruby-paths --depth 7

Reads a complete RGBA32F optical packet; writes PREFIX.bin and PREFIX.json.
No shader compilation, rendering, processes, or production asset writes occur.
The input must isolate control instance zero. Runtime keeps the captured scale,
IOR, roughness and shape; translation, live lights, absorption and SSLE are
evaluated by the separate experimental shader. This is not a color/image atlas.

Public API: read_rows(path), load_fixture(rows), compile_paths(rows, depth=7).
compile_paths returns (packed_bytes, metadata). Metadata offsets count vec4 rows
relative to packed_bytes. Program zero means use the original ray tracer.
"""

import argparse
import array
from collections import Counter, deque
from dataclasses import dataclass, field
from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path
import struct
import sys
import time


MAX_BYTES = 32 * 1024 * 1024
STEP_STRIDE = 10
SAMPLES = 2
MAX_DEPTH = 16
MAX_EVENTS = 64
BARY_GUARD = 2e-5
DISTANCE_GUARD = 2e-8
# The source retains weights strictly above 5e-5. Keep additional headroom for
# float32 coefficient rounding; this is a bound, not a changed render cutoff.
CERTIFIED_TAIL_CUTOFF = 4e-5


def dot(a, b): return sum(x*y for x, y in zip(a, b))
def add(a, b): return tuple(x+y for x, y in zip(a, b))
def sub(a, b): return tuple(x-y for x, y in zip(a, b))
def mul(a, s): return tuple(x*s for x in a)
def cross(a, b): return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])
def length(a): return math.sqrt(max(dot(a, a), 0.))
def unit(a): return mul(a, 1./max(length(a), 1e-7))
def clamp(x, lo=0., hi=1.): return min(max(x, lo), hi)
def fract(x): return x-math.floor(x)
def f32(x): return struct.unpack('<f', struct.pack('<f', x))[0]


def read_rows(path):
    """Return float32 tuples from the complete little-endian optical buffer."""
    raw = Path(path).read_bytes()
    if len(raw) % 16 or len(raw) < 256*16:
        raise ValueError('frame must be a complete vec4-aligned optical buffer')
    return list(struct.iter_unpack('<4f', raw))


def index(value, limit):
    if not math.isfinite(value) or value != int(value) or not 0 <= value < limit:
        raise ValueError('invalid optical packet address')
    return int(value)


@dataclass(frozen=True)
class Triangle:
    address: int
    rank: int
    material: int
    a: tuple
    e1: tuple
    e2: tuple
    normal: tuple

    @property
    def vertices(self): return (self.a, add(self.a, self.e1), add(self.a, self.e2))


def tree_triangles(rows, root, end):
    root, end = index(root, len(rows)), index(end, len(rows))
    if end <= root or (end-root) % 3:
        raise ValueError('invalid control BVH interval')
    result = []
    for node in range(root, end, 3):
        count = index(rows[node+2][0], 5)
        if not count: continue
        first = index(rows[node+1][3], len(rows))
        if first+count*6 > len(rows): raise ValueError('truncated triangle leaf')
        for offset in range(count):
            at = first+offset*6
            a, b, c, n, n1, n2 = rows[at:at+6]
            if not all(math.isfinite(value) for row in (a,b,c,n,n1,n2) for value in row):
                raise ValueError('nonfinite control geometry')
            material=index(a[3],256)
            if n[3] < .5 or n[:3] != n1[:3] or n[:3] != n2[:3]:
                raise ValueError('compiled paths require original flat triangle normals')
            if length(n[:3])<1e-7: raise ValueError('invalid flat triangle normal')
            e1, e2 = b[:3], c[:3]
            if b[3] < .5: e1, e2 = sub(e1, a[:3]), sub(e2, a[:3])
            if length(cross(e1, e2)) < 1e-16:
                raise ValueError('degenerate triangle in compiled control')
            result.append(Triangle(at, len(result), material, a[:3], e1, e2, unit(n[:3])))
    if not result: raise ValueError('empty control tree')
    return tuple(result)


def geometric_key(triangle):
    return (triangle.a, triangle.e1, triangle.e2, triangle.normal, triangle.material)


def load_fixture(rows):
    """Validate the deliberately isolated, orthographic instance-zero fixture."""
    if rows[12][0] != 1. or rows[0][1] != 0.:
        raise ValueError('fixture must expose only instance zero and zero panel instances')
    disabled = [(234,0,'bubbles'), (240,0,'spheres'), (245,0,'custom panels'),
                (248,0,'timeline meshes'), (232,2,'navigation'), (247,1,'lane monitors'),
                (250,3,'ruler'), (252,3,'playhead')]
    for row, word, name in disabled:
        if rows[row][word] != 0.:
            raise ValueError(f'fixture must disable {name}')
    if rows[1][3] != 0. or rows[4][:3] != (0.,0.,-1.) or rows[2][:3] != (1.,0.,0.) or rows[3][:3] != (0.,1.,0.):
        raise ValueError('fixture requires the canonical down-Z orthographic camera')
    if rows[0][2] != SAMPLES or rows[8][2:] != (float(MAX_DEPTH),float(MAX_EVENTS)):
        raise ValueError('fixture requires two samples, depth 16 and 64 events')
    pose = rows[192]
    if not all(math.isfinite(v) for v in pose) or pose[3] <= 0.:
        raise ValueError('invalid instance-zero uniform pose')
    entry = tree_triangles(rows, rows[193][0], rows[193][1])
    materials = {t.material for t in entry}
    if len(entry) != 76 or len(materials) != 1:
        raise ValueError('first prototype accepts the single-material 76-triangle ruby only')
    material = materials.pop()
    base = int(rows[193][3]) if rows[193][3] >= .5 else int(rows[12][1])
    base += 40*material
    if base < 256 or base+40 > len(rows): raise ValueError('invalid ruby material range')
    for row in rows[base:base+40]:
        if not all(math.isfinite(v) for v in row): raise ValueError('nonfinite ruby material')
    attenuation_parameters=(*rows[base+1][:3],rows[214][3],*rows[215][2:])
    if not all(math.isfinite(value) and value>=0. for value in attenuation_parameters):
        raise ValueError('tail certification requires nonnegative Beer and engraving attenuation')
    if min(rows[base+4][:3])<0.:
        raise ValueError('tail certification requires nonnegative transmission color')
    if not (1. < rows[base][1] < 3. and 0. < rows[base][2] < .1 and rows[base][3] > .999):
        raise ValueError('fixture material is not the authored transparent ruby')
    interior = rows[base+2]
    if interior[2] > 0. or interior[0] <= 0.:
        raise ValueError('expected the ruby material-only triangle exit tree')
    inside = tree_triangles(rows, interior[0], interior[1])
    if Counter(map(geometric_key, inside)) != Counter(map(geometric_key, entry)):
        raise ValueError('entry and interior trees do not contain identical original geometry')
    vertices = [v for t in entry for v in t.vertices]
    lo = tuple(min(v[i] for v in vertices) for i in range(3))
    hi = tuple(max(v[i] for v in vertices) for i in range(3))
    # The subsequent cell checks use every actual triangle, with no fitted
    # construction planes. Record convexity only as a diagnostic: authored
    # triangulation can depart from a convex hull. Every accepted cell must
    # still prove nearest-hit ordering and exterior self-misses against ALL
    # original triangles; certify() does not depend on global convexity.
    residual = 0.
    for triangle in entry:
        normal = unit(cross(triangle.e1, triangle.e2))
        if dot(normal, triangle.normal) < 0.: normal = mul(normal,-1.)
        residual = max(residual, max(dot(normal, sub(v,triangle.a)) for v in vertices))
    half = rows[base+3][:3]
    epsilon_world = max(2e-7, min(half)*1e-5)
    epsilon_local = epsilon_world/pose[3]
    signature = hashlib.sha256()
    for triangle in entry:
        for vector in (triangle.a,triangle.e1,triangle.e2,triangle.normal):
            signature.update(struct.pack('<3d',*vector))
    return {'entry':entry, 'inside':inside, 'material':material, 'base':base,
            'scale':pose[3], 'epsilon_world':epsilon_world, 'epsilon_local':epsilon_local,
            'minimum_distance':1e-7/pose[3], 'lo':lo, 'hi':hi,
            'bounds':[lo[0],lo[1],hi[0],hi[1]], 'primary_origin_z':hi[2]+max(hi[0]-lo[0],hi[1]-lo[1]),
            'geometry_signature':signature.hexdigest(), 'convex_residual_m':residual,
            'material_rows':rows[base:base+40]}


# Scalar affine expressions use (x coefficient, y coefficient, constant).
def affine_value(a, point): return a[0]*point[0]+a[1]*point[1]+a[2]
def affine_point(origin, point): return tuple(affine_value(a,point) for a in origin)
def affine_shift(origin, direction, distance):
    return tuple(add(a, (0.,0.,direction[i]*distance)) for i,a in enumerate(origin))
def affine_hit(origin, direction, distance):
    return tuple(add(a,mul(distance,direction[i])) for i,a in enumerate(origin))


def serialization_error(expression, point):
    """Bound coefficient quantization and one float32 affine evaluation.

    The topology proof is in float64. Reject ill-conditioned maps whose stored
    coefficients could move the point outside its numerical distance guard.
    This does not assert bit equality of GPU transcendental implementations.
    """
    try:
        rounded=tuple(f32(value) for value in expression)
    except OverflowError:
        return math.inf
    if not all(math.isfinite(value) for value in rounded): return math.inf
    terms=(rounded[0]*point[0],rounded[1]*point[1],rounded[2])
    quantization=abs(affine_value(sub(rounded,expression),point))
    evaluation=(3.*2.**-24)/(1.-3.*2.**-24)*sum(abs(term) for term in terms)
    return quantization+evaluation


@lru_cache(maxsize=8192)
def intersection_affines(triangle, origin, ray, inside):
    cosine = dot(ray,triangle.normal)
    if (inside and cosine <= 0.) or (not inside and cosine >= 0.): return None
    h = cross(ray,triangle.e2)
    determinant = dot(triangle.e1,h)
    if abs(determinant) < 1e-12: return None
    offset = tuple(sub(a,(0.,0.,triangle.a[i])) for i,a in enumerate(origin))
    # Dot/cross are linear, so evaluate their three coefficient columns.
    columns = [tuple(offset[i][column] for i in range(3)) for column in range(3)]
    u = tuple(dot(column,h)/determinant for column in columns)
    q = [cross(column,triangle.e1) for column in columns]
    v = tuple(dot(ray,column)/determinant for column in q)
    distance = tuple(dot(triangle.e2,column)/determinant for column in q)
    return u,v,distance


def nearest(triangles, origin, ray, point, inside, minimum):
    chosen = None
    for triangle in triangles:
        values = intersection_affines(triangle,origin,ray,inside)
        if values is None: continue
        u,v,d = (affine_value(a,point) for a in values)
        if u < 0. or v < 0. or u+v > 1. or d <= minimum: continue
        if chosen is None or d < chosen[2]: chosen = triangle,values,d
    return chosen


def fresnel(cosine, eta_i, eta_t):
    ci=clamp(abs(cosine)); ratio=eta_i/eta_t
    sin_t2=ratio*ratio*(1.-ci*ci)
    if sin_t2 >= 1.: return 1.
    ct=math.sqrt(max(0.,1.-sin_t2))
    rs=(eta_i*ci-eta_t*ct)/max(eta_i*ci+eta_t*ct,1e-7)
    rp=(eta_t*ci-eta_i*ct)/max(eta_t*ci+eta_i*ct,1e-7)
    return .5*(rs*rs+rp*rp)


def smith(cosine,alpha):
    c=clamp(abs(cosine))
    return 2.*c/max(c+math.sqrt(alpha*alpha+(1.-alpha*alpha)*c*c),1e-7)


def sample_mask(incoming,outgoing,alpha):
    gi,go=smith(incoming,alpha),smith(outgoing,alpha)
    return go/max(gi+go-gi*go,1e-7)


def micro_normal(n,view,roughness,u):
    alpha=max(roughness*roughness,1e-5)
    tangent=unit(cross(n,(0.,0.,1.))) if abs(n[2]) <= .999 else (1.,0.,0.)
    bitangent=cross(n,tangent)
    v=unit((alpha*dot(view,tangent),alpha*dot(view,bitangent),max(dot(view,n),1e-5)))
    lensq=v[0]*v[0]+v[1]*v[1]
    t1=mul((-v[1],v[0],0.),1./math.sqrt(lensq)) if lensq > 1e-7 else (1.,0.,0.)
    t2=cross(v,t1); r=math.sqrt(u[0]); phi=6.28318530718*u[1]
    x,y=r*math.cos(phi),r*math.sin(phi); s=.5*(1.+v[2])
    y=(1.-s)*math.sqrt(max(0.,1.-x*x))+s*y
    nh=add(add(mul(t1,x),mul(t2,y)),mul(v,math.sqrt(max(0.,1.-x*x-y*y))))
    local=unit((alpha*nh[0],alpha*nh[1],max(0.,nh[2])))
    return unit(add(add(mul(tangent,local[0]),mul(bitangent,local[1])),mul(n,local[2])))


def material_event(fixture, ray, outward, inside, sample, event):
    material=fixture['material_rows']; roughness=material[0][2]; ior=material[0][1]
    n=mul(outward,-1.) if inside else outward
    random=(fract(.324717957+.618033989*sample),fract(.754877666+.414213562*sample))
    xi=(fract(random[0]+event*.754877666),fract(random[1]+event*.569840296))
    micro=micro_normal(n,mul(ray,-1.),roughness,xi)
    eta_i,eta_t=(ior,1.) if inside else (1.,ior)
    f=fresnel(dot(mul(ray,-1.),micro),eta_i,eta_t)
    reflected=sub(ray,mul(micro,2.*dot(micro,ray)))
    eta=eta_i/eta_t; ni=dot(micro,ray); k=1.-eta*eta*(1.-ni*ni)
    transmitted=(0.,0.,0.) if k < 0. else sub(mul(ray,eta),mul(micro,eta*ni+math.sqrt(k)))
    alpha=max(roughness*roughness,1e-5)
    cosine=dot(mul(ray,-1.),n); coordinate=clamp(abs(cosine))*15.
    low=int(math.floor(coordinate)); high=min(low+1,15); table=36 if inside else 32
    a=material[table+low//4][low%4]; b=material[table+high//4][high%4]
    energy=a*(1.-fract(coordinate))+b*fract(coordinate)
    if energy <= 0.: raise ValueError('invalid GGX energy table')
    reflect_factor=f*sample_mask(cosine,dot(reflected,n),alpha)/energy if dot(reflected,n)>0. else 0.
    transmit_factor=((1.-f)*material[0][3]*eta_i*eta_i/(eta_t*eta_t))*sample_mask(cosine,dot(transmitted,n),alpha)/energy if dot(transmitted,n)<0. else 0.
    return reflected,transmitted,(reflect_factor,)*3,mul(material[4][:3],transmit_factor)


def clip_polygon(polygon, plane):
    """Keep affine plane>=0. No tolerance silently enlarges acceptance."""
    if not polygon: return []
    result=[]; previous=polygon[-1]; old=affine_value(plane,previous)
    for current in polygon:
        new=affine_value(plane,current)
        if (old>=0.) != (new>=0.):
            ratio=old/(old-new)
            result.append((previous[0]+ratio*(current[0]-previous[0]),previous[1]+ratio*(current[1]-previous[1])))
        if new>=0.: result.append(current)
        previous,old=current,new
    return result


def intersects_region(values, corners, minimum, selected_distance=None):
    u,v,d=values
    planes=[add(u,(0.,0.,BARY_GUARD)),add(v,(0.,0.,BARY_GUARD)),
            sub((0.,0.,1.+BARY_GUARD),add(u,v)), sub(d,(0.,0.,minimum-DISTANCE_GUARD))]
    if selected_distance is not None:
        planes.append(add(sub(selected_distance,d),(0.,0.,DISTANCE_GUARD)))
    polygon=list(corners)
    for plane in planes:
        values_at_corners=[affine_value(plane,p) for p in corners]
        if max(values_at_corners)<0.: return False
        if min(values_at_corners)>=0.: continue
        polygon=clip_polygon(polygon,plane)
        if not polygon: return False
    return bool(polygon)


@dataclass
class Step:
    hit: tuple
    distance: tuple
    inside: bool
    incident: tuple
    outward: tuple
    reflected: tuple
    transmitted: tuple
    reflect_coefficient: tuple
    transmit_coefficient: tuple
    event: int
    triangle: int
    uv: tuple
    competitors: tuple = field(repr=False)
    external_queries: tuple = field(repr=False)

    def rows(self):
        return [tuple(a)+(0.,) for a in self.hit] + [self.distance+(float(self.inside),),
            self.incident+(float(self.event),), self.outward+(0.,), self.reflected+(0.,),
            self.transmitted+(0.,), self.reflect_coefficient+(0.,), self.transmit_coefficient+(0.,)]


@dataclass
class Program:
    sample: int
    steps: tuple
    signature: tuple


def compile_chain(fixture, point, sample):
    origin=((1.,0.,0.),(0.,1.,0.),(0.,0.,fixture['primary_origin_z']))
    ray=(0.,0.,-1.); steps=[]; inside=False; maximum_weight=(1.,1.,1.)
    for event in range(MAX_DEPTH):
        triangles=fixture['inside'] if inside else fixture['entry']
        hit=nearest(triangles,origin,ray,point,inside,fixture['minimum_distance'])
        if hit is None: return None
        triangle,(u,v,distance),_=hit
        position=affine_hit(origin,ray,distance)
        reflected,transmitted,rc,tc=material_event(fixture,ray,triangle.normal,inside,sample,event)
        competitors=tuple(values for other in triangles if other.address!=triangle.address
            if (values:=intersection_affines(other,origin,ray,inside)) is not None)
        external=transmitted if inside else reflected
        external_coefficient=tc if inside else rc
        queries=()
        if event+1<MAX_DEPTH and max(external_coefficient)>0.:
            external_origin=affine_shift(position,external,fixture['epsilon_local'])
            queries=tuple(values for other in fixture['entry']
                if (values:=intersection_affines(other,external_origin,external,False)) is not None)
        steps.append(Step(position,distance,inside,ray,triangle.normal,reflected,transmitted,rc,tc,
                          event,triangle.address,(u,v),competitors,queries))
        continuing=rc if inside else tc
        if event+1>=MAX_DEPTH or max(continuing)<=0.: break
        # Beer and SSLE transmission cannot increase a channel. Ignoring both
        # gives a uniform upper bound independent of XY, lights and glyphs.
        # Round upward and inflate each coefficient before using this only to
        # omit a tail that the unchanged runtime cutoff must already discard.
        maximum_weight=tuple(math.nextafter(weight*(max(coefficient,f32(coefficient))*(1.+1e-5)+1e-12),math.inf)
            for weight,coefficient in zip(maximum_weight,continuing))
        if max(maximum_weight)<=CERTIFIED_TAIL_CUTOFF: break
        ray=reflected if inside else transmitted
        origin=affine_shift(position,ray,fixture['epsilon_local']); inside=True
    signature=(sample,tuple(step.triangle for step in steps))
    return Program(sample,tuple(steps),signature)


def certify(program, corners, fixture):
    """Certify every triangle and external self-miss over the entire XY cell.

    All intersection/barycentric functions are affine for a fixed path. Corner
    checks establish the chosen triangle's interior. Polygon clipping rejects
    cells where ANY other original triangle could be nearer or an exterior
    branch could re-enter. Failure always means subdivision or program zero.
    """
    if program is None: return False
    minimum=fixture['minimum_distance']
    for step in program.steps:
        u,v=step.uv
        for point in corners:
            a,b,d=affine_value(u,point),affine_value(v,point),affine_value(step.distance,point)
            if a<BARY_GUARD or b<BARY_GUARD or a+b>1.-BARY_GUARD or d<minimum+DISTANCE_GUARD:
                return False
            for expression in (*step.hit,step.distance):
                if serialization_error(expression,point)>DISTANCE_GUARD:
                    return False
        for values in step.competitors:
            if intersects_region(values,corners,minimum,step.distance): return False
        for values in step.external_queries:
            if intersects_region(values,corners,minimum): return False
    return True


def compile_paths(rows, depth=7, budget_seconds=40.):
    if not 1<=depth<=8: raise ValueError('quadtree depth must be 1..8')
    if not math.isfinite(budget_seconds) or not 0<=budget_seconds<=120:
        raise ValueError('preprocessing budget must be between 0 and 120 seconds')
    intersection_affines.cache_clear()
    fixture=load_fixture(rows); edge=1<<depth; bounds=fixture['bounds']
    maps=array.array('I',[0])*(SAMPLES*edge*edge)
    programs=[None]; cache={}; statistics=Counter(); table_rows=SAMPLES*edge*edge//4
    stored_step_rows=0
    started=time.monotonic(); deadline=started+budget_seconds; next_progress=started+5.
    # Breadth-first visits alternate sample roots and cover the whole shape
    # before refining. Expiry leaves only unproven cells at canonical fallback.
    pending=deque((sample,0,0,edge,None) for sample in range(SAMPLES))

    def corners(x,y,size):
        left=bounds[0]+(bounds[2]-bounds[0])*x/edge
        right=bounds[0]+(bounds[2]-bounds[0])*(x+size)/edge
        bottom=bounds[1]+(bounds[3]-bounds[1])*y/edge
        top=bounds[1]+(bounds[3]-bounds[1])*(y+size)/edge
        return ((left,bottom),(right,bottom),(right,top),(left,top))

    def save(program):
        nonlocal stored_step_rows
        known=cache.get(program.signature)
        if known is not None: return known
        required=table_rows+len(programs)+1+stored_step_rows+len(program.steps)*STEP_STRIDE
        if required*16>MAX_BYTES:
            statistics['table_limit_rejections']+=1
            return 0
        identifier=len(programs); programs.append(program); cache[program.signature]=identifier
        stored_step_rows+=len(program.steps)*STEP_STRIDE
        return identifier

    def visit(sample,x,y,size,preferred=None):
        statistics['quadtree_nodes']+=1
        box=corners(x,y,size); program=preferred
        good=certify(program,box,fixture)
        if not good:
            center=((box[0][0]+box[2][0])*.5,(box[0][1]+box[2][1])*.5)
            candidate=compile_chain(fixture,center,sample)
            statistics['center_paths']+=1
            if candidate is not None:
                existing=cache.get(candidate.signature)
                program=programs[existing] if existing is not None else candidate
                good=certify(program,box,fixture)
            else: program=None
        if good:
            identifier=save(program)
            if identifier:
                statistics['accepted_cells']+=1
                statistics[f'accepted_texels_sample_{sample}']+=size*size
                for row in range(y,y+size):
                    start=sample*edge*edge+row*edge+x
                    maps[start:start+size]=array.array('I',[identifier])*size
                return
        if size==1:
            statistics['fallback_texels']+=1
            return
        half=size//2
        for dy in (0,half):
            for dx in (0,half): pending.append((sample,x+dx,y+dy,half,program))

    while pending and time.monotonic()<deadline:
        visit(*pending.popleft())
        now=time.monotonic()
        if now>=next_progress:
            print(json.dumps({'phase':'path_preprocessing','seconds':round(now-started,1),
                'visited_cells':statistics['quadtree_nodes'],'programs':len(programs)-1,
                'pending_cells':len(pending)}),flush=True)
            next_progress=now+5.
    statistics['budget_unvisited_texels']=sum(size*size for _,_,_,size,_ in pending)
    statistics['fallback_texels']+=statistics['budget_unvisited_texels']
    statistics['budget_exhausted']=int(bool(pending))
    statistics['intersection_cache_hits']=intersection_affines.cache_info().hits
    statistics['intersection_cache_misses']=intersection_affines.cache_info().misses
    elapsed=time.monotonic()-started
    pending.clear()
    intersection_affines.cache_clear()
    map_base=0; program_base=table_rows; step_base=program_base+len(programs)
    packed=array.array('f',map(float,maps))
    packed.extend([0.]*(len(programs)*4))
    cursor=step_base
    for identifier,program in enumerate(programs[1:],1):
        at=(program_base+identifier)*4
        packed[at:at+4]=array.array('f',[float(cursor),float(len(program.steps)),0.,0.])
        for step in program.steps:
            for row in step.rows(): packed.extend(row)
        cursor+=len(program.steps)*STEP_STRIDE
    if len(packed)*4>MAX_BYTES: raise ValueError('compiled path table exceeded 32 MiB')
    if not all(math.isfinite(value) for value in packed): raise ValueError('nonfinite compiled path coefficient')
    if sys.byteorder!='little': packed.byteswap()
    frozen={str(i):list(rows[i]) for i in range(fixture['base'],fixture['base']+40)}
    frozen['193']=list(rows[193])
    metadata={
        'schema':1, 'kind':'private_piecewise_affine_gem_paths', 'map_base':map_base,
        'program_base':program_base, 'step_base':step_base, 'grid_size':edge,
        'program_count':len(programs), 'step_stride':STEP_STRIDE, 'bounds':bounds,
        'map_order':'sample-major, local-Y ascending rows, local-X ascending columns; 4 float IDs per vec4',
        'pose_row':192, 'instance':0, 'medium':4+fixture['material'],
        'material_row_base':fixture['base'], 'scale':fixture['scale'],
        'epsilon_world':fixture['epsilon_world'], 'epsilon_local':fixture['epsilon_local'],
        'primary_origin_z':fixture['primary_origin_z'], 'sample_count':SAMPLES,
        'expected_depth':MAX_DEPTH, 'expected_events':MAX_EVENTS,
        'geometry_signature':fixture['geometry_signature'], 'frozen_rows':frozen,
        'convex_residual_m':fixture['convex_residual_m'], 'barycentric_guard':BARY_GUARD,
        'distance_guard_m':DISTANCE_GUARD, 'maximum_bytes':MAX_BYTES,
        'certified_tail_cutoff':CERTIFIED_TAIL_CUTOFF,
        'preprocessing_budget_seconds':budget_seconds, 'preprocessing_seconds':elapsed,
        'bytes':len(packed)*4, 'statistics':dict(statistics),
        'affine_order':['x_coefficient','y_coefficient','constant','metadata'],
        'step_rows':['hit_x','hit_y','hit_z','local_distance;w=inside',
                     'incident;w=event','outward_normal','reflected','transmitted',
                     'reflection_coefficient','transmission_coefficient'],
        'runtime_order':'For inside events apply existing SSLE then Beer; apply stored branch coefficients; retain branches only at depth+1<16 and max RGB weight>0.00005. Evaluate external reflection at entry, external transmission thereafter; continue entry transmission then interior reflection.',
        'scope':'Isolated flat-faceted ruby; fixed scale/IOR/roughness/sample context. Global convexity is not assumed: nearest hits and external self-misses are certified against all original triangles per cell. A uniform upper bound may omit a tail already below the original throughput cutoff; Beer/SSLE attenuation is ignored by that bound. Live external lights/floor and glyph density are not baked.',
        'precision':'Compiler arithmetic is float64 and coefficients serialize as float32. Each accepted cell bounds coefficient quantization plus affine float32 evaluation error by distance_guard_m. Expressions mirror the source but world-to-local cancellation and GPU transcendental differences still require measured image validation.',
        'fallback':'ProgramID0, uncertified cells, incompatible frame/material/scale or any invalid lookup use canonical gemTrace.',
        'validation':'Source generation only. GPU numerical/image equivalence, accepted-pixel coverage and performance remain unverified.',
    }
    return packed.tobytes(),metadata


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--frame',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--depth',type=int,default=7)
    parser.add_argument('--budget-seconds',type=float,default=40.)
    args=parser.parse_args()
    production=Path(__file__).resolve().parents[2]/'Procedures'
    for path in (args.output.with_suffix('.bin'),args.output.with_suffix('.json')):
        resolved=path.resolve()
        if resolved==args.frame.resolve() or production in resolved.parents:
            raise ValueError('private compiler cannot overwrite its input or production assets')
    raw=args.frame.read_bytes(); rows=read_rows(args.frame)
    packet,metadata=compile_paths(rows,args.depth,args.budget_seconds)
    metadata.update({'source_frame':str(args.frame.resolve()),
        'source_frame_sha256':hashlib.sha256(raw).hexdigest(),
        'compiler_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'packet_sha256':hashlib.sha256(packet).hexdigest()})
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.with_suffix('.bin').write_bytes(packet)
    args.output.with_suffix('.json').write_text(json.dumps(metadata,indent=2)+'\n')
    print(json.dumps({'metadata':str(args.output.with_suffix('.json')),
        'program_count':metadata['program_count'],'bytes':metadata['bytes'],
        'statistics':metadata['statistics']}))


if __name__=='__main__': main()
