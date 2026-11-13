#!/usr/bin/env python3
"""Private GPU-baked optical response for one fixed-view ruby.

prepare emits an instrumented canonical shader; response_probe executes its bake.
evaluate emits a lookup shader and appends the immutable response to a fixture.
No production shader, app, or source mesh is overwritten. The first experiment
fixes glyph/shape/material/view; direct terminal lighting remains live. Engraving
uses four linear light-response bases, with an approximate geometric correction
for moved lights. This is deliberately approximate, not a topology certificate.
"""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import struct
import sys

from response_floor import WGSL_FLOOR

LOBES = 8
SAMPLE_ROWS = 30
PIXEL_ROWS = 60
MAX_BYTES = 128 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def row(data, index):
    return list(struct.unpack_from('<4f', data, index * 16))


def literal(value):
    require(math.isfinite(value), 'Nonfinite constant')
    text = format(value, '.9g')
    return text if '.' in text or 'e' in text.lower() else text + '.0'


def vector(values):
    return 'vec3<f32>(' + ','.join(map(literal, values)) + ')'


def function(source, name):
    start = source.index('fn ' + name + '(')
    opening = source.index('{', start)
    level = 1
    end = opening + 1
    while level:
        require(end < len(source), 'Unclosed WGSL function')
        level += (source[end] == '{') - (source[end] == '}')
        end += 1
    return start, end, source[start:end]


def replace_once(source, old, new):
    require(source.count(old) == 1, 'Canonical marker changed: ' + old[:100])
    return source.replace(old, new)


def validate_fixture(data):
    require(49408 * 16 <= len(data) <= MAX_BYTES and len(data) % 16 == 0, 'Invalid optical packet')
    require(row(data, 0)[1:3] == [0., 2.] and row(data, 12)[0] == 1., 'Requires isolated instance-zero ruby')
    require(row(data, 8)[2:] == [16., 64.] and row(data, 9)[0] == 3., 'Canonical quality/light count changed')
    require(row(data, 10)[1] == 0., 'Isolated floor requires the old visibility field disabled')
    require(row(data, 1)[3] == 0. and row(data, 2)[:3] == [1., 0., 0.]
            and row(data, 3)[:3] == [0., 1., 0.] and row(data, 4)[:3] == [0., 0., -1.], 'Requires fixed down-Z orthographic view')
    require(all(row(data, i)[0] == 0. for i in (234, 240, 245, 248))
            and row(data, 245)[2] == 0. and row(data, 250)[3] == 0.
            and row(data, 252)[3] == 0. and row(data, 232)[1:3] == [0., 0.]
            and row(data, 247)[1] == 0., 'Other geometry invalidates this isolated response')
    values = array.array('f')
    values.frombytes(data)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(x) for x in values), 'Nonfinite optical packet')


def output_paths(args):
    source = {args.shader.resolve(), args.frame.resolve()}
    source.update(getattr(args, x).resolve() for x in ('atlas', 'metadata') if hasattr(args, x))
    for suffix in ('.wgsl', '.bin', '.json'):
        path = args.output.with_suffix(suffix).resolve()
        require(path not in source and 'Procedures' not in path.parts,
                'Private response output must not overwrite inputs or production assets')
    args.output.parent.mkdir(parents=True, exist_ok=True)


BAKE_SUPPORT = r'''
// Optical-response bake: only this offline stage traces the glass paths.
var<private> responsePositions:array<vec3<f32>,8>;
var<private> responseDirections:array<vec3<f32>,8>;
var<private> responseWeights:array<vec3<f32>,8>;
var<private> responseBases:array<vec3<f32>,4>;
var<private> responseHead:vec4<f32>;
var<private> responseDropped:vec3<f32>;
var<private> responseLeaves:i32;

fn responseRecord(origin:vec3<f32>, direction:vec3<f32>, weight:vec3<f32>) {
    var slot:i32=0;
    for(var i:i32=1;i<8;i++){if(gemMax(responseWeights[i])<gemMax(responseWeights[slot])){slot=i;}}
    if(gemMax(weight)>gemMax(responseWeights[slot])){
        responseDropped+=responseWeights[slot];
        responseWeights[slot]=weight;
        let pose=gemRow(192);
        responsePositions[slot]=(origin-pose.xyz)/pose.w;
        responseDirections[slot]=direction;
    }else{responseDropped+=weight;}
    responseLeaves+=1;
}
fn responseTerminal(origin:vec3<f32>,direction:vec3<f32>,weight:vec3<f32>)->vec4<f32> {
    // No other objects exist in this fixture. Preserve self-reentry branches;
    // do not bake the current emitter/floor colors into terminal radiance.
    if(gemAny(origin,direction,1e20)){return vec4<f32>(0.0);}
    responseRecord(origin,direction,weight);
    return vec4<f32>(0.0,0.0,0.0,1.0);
}
fn responseEngraving(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32,pathWeight:vec3<f32>)->f32 {
    if(medium<4||medium>=260||gemRow(244).y<0.5||abs(ray.z)<0.00001){return 1.0;}
    let base=gemPanelRow(medium);let descriptor=i32(gemRow(base+2).w);
    if(descriptor==0||gemRow(base).w<0.0001){return 1.0;}
    let pose=gemRow(192);let local=(origin-pose.xyz)/pose.w;let limit=farLimit/pose.w;
    let region=gemRow(descriptor);let depth=gemRow(descriptor+1);
    let pixel=RESPONSE_FOOTPRINT;
    let center=vec3<f32>(region.xy+region.zw*0.5,(depth.x+depth.z)*0.5);
    let extent=vec3<f32>(region.zw*0.5+vec2<f32>(pixel),(depth.x-depth.z)*0.5+0.0000001);
    if(!gemBox(local,gemInverseRay(ray),center,extent,limit)){return 1.0;}
    let sigma=gemRow(base+1).xyz;
    var transmission:f32=1.0;
    for(var visit:i32=0;visit<3;visit++){
        var layer=visit;if(ray.z>0.0){layer=2-visit;}
        var z=depth.x;var weight:f32=1.0;var shift=vec2<f32>(0.0);
        if(layer==1){z=depth.y;weight=gemRow(215).z;shift=gemRow(216).xy;}
        if(layer==2){z=depth.z;weight=gemRow(215).w;shift=gemRow(216).zw;}
        let t=(z-local.z)/ray.z;if(t<=0.0000001||t>=limit){continue;}
        let point=local+ray*t;
        let density=gemControlDensity(point.xy,descriptor,shift,pixel*0.75,layer);
        if(density>0.0001){
            let alpha=1.0-exp(-gemRow(214).w*weight*min(density,1.0)/max(abs(ray.z),0.12));
            let coefficient=pathWeight*transmission*alpha*exp(-sigma*t)*gemRow(214).xyz*gemRow(223).w;
            responseBases[0]+=coefficient;
            let worldPoint=pose.xyz+point*pose.w;
            for(var light:i32=0;light<3;light++){
                let at=16+5*light;let toLight=gemRow(at).xyz-worldPoint;
                let distance=length(toLight);let l=toLight/max(distance,0.000001);
                let cosine=max(dot(gemRow(at+3).xyz,-l),0.0);
                let exitHit=gemControlExit(worldPoint,l,medium,distance);
                if(cosine>0.0&&exitHit.panel==medium&&exitHit.distance>0.0){
                    let area=4.0*gemRow(at+1).w*gemRow(at+2).w;
                    let transmit=1.0-gemFresnel(dot(l,exitHit.normal),1.0,gemRow(base).y);
                    responseBases[light+1]+=coefficient*exp(-sigma*(exitHit.distance/pose.w))
                        *(transmit*cosine*area/(3.14159265359*distance*distance));
                }
            }
            transmission*=1.0-alpha;
        }
    }
    return transmission;
}
'''


def prepare(args):
    require(args.edge in (128, 256), 'First response fixture supports 128/256 square atlases')
    data = args.frame.read_bytes()
    validate_fixture(data)
    original = args.shader.read_text()
    pose = row(data, 192)
    tree = row(data, 193)
    require(tree[0].is_integer() and 256 <= tree[0] < len(data)//16-2, 'Invalid ruby tree')
    low, high = row(data, int(tree[0]))[:3], row(data, int(tree[0])+1)[:3]
    margin = max(high[i]-low[i] for i in (0, 1)) * 2 / args.edge
    bounds = [low[0]-margin, low[1]-margin, high[0]+margin, high[1]+margin]
    footprint = 2 * row(data, 3)[3] / (row(data, 8)[1] * pose[3])
    support = BAKE_SUPPORT.replace('RESPONSE_FOOTPRINT', literal(footprint))
    _, _, trace = function(original, 'gemTrace')
    trace = trace.replace('fn gemTrace(', 'fn responseTrace(', 1)
    trace = replace_once(trace,
        'if(current.medium<0){environment=gemEnvironment(current.origin,current.direction);}',
        '// Bake intrinsic geometry independently of current floor/emitter occlusion.')
    trace = replace_once(trace, 'var hit:GemHit=gemNearest(current.origin,current.direction,current.medium,environment.w);',
        'var hit:GemHit=gemNearest(current.origin,current.direction,current.medium,environment.w);\n'
        'if(event==0&&hit.panel>=0&&hit.distance>0.0){responseHead=vec4<f32>((current.origin+current.direction*hit.distance-gemRow(192).xyz)/gemRow(192).w,1.0);}')
    trace = replace_once(trace, 'result+=current.weight*environmentRadiance;',
                         'responseRecord(current.origin,current.direction,current.weight);')
    trace = replace_once(trace,
        'var engraving:vec4<f32>=gemControlEngraving(current.origin,current.direction,current.medium,hit.distance);',
        'var engraving:vec4<f32>=vec4<f32>(0.0,0.0,0.0,responseEngraving(current.origin,current.direction,current.medium,hit.distance,current.weight));')
    for name, weight in (('reflected', 'reflectWeight'), ('transmitted', 'transmitWeight')):
        old = f'gemTerminal(point+{name}*epsilon,{name})'
        require(trace.count(old) == 2, 'Terminal call layout changed')
        trace = trace.replace(old, f'responseTerminal(point+{name}*epsilon,{name},{weight})')
    bake = f'''
fn responseBake(screen:vec2<f32>,pixelIndex:u32) {{
    let xy=vec2<f32>({literal(bounds[0])},{literal(bounds[1])})+(screen+vec2<f32>(1.0))*0.5*
        vec2<f32>({literal(bounds[2]-bounds[0])},{literal(bounds[3]-bounds[1])});
    let pose=gemRow(192);
    for(var sampleIndex:i32=0;sampleIndex<2;sampleIndex++){{
        for(var i:i32=0;i<8;i++){{responsePositions[i]=vec3<f32>(0.0);responseDirections[i]=vec3<f32>(0.0);responseWeights[i]=vec3<f32>(0.0);}}
        for(var i:i32=0;i<4;i++){{responseBases[i]=vec3<f32>(0.0);}}
        responseHead=vec4<f32>(0.0);responseDropped=vec3<f32>(0.0);responseLeaves=0;
        let origin=vec3<f32>(pose.xy+xy*pose.w,gemRow(1).z);
        let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sampleIndex));
        let ignored=responseTrace(origin,vec3<f32>(0.0,0.0,-1.0),random);
        let base=pixelIndex*60u+u32(sampleIndex)*30u;
        workstationComputeOutput.pixels[base]=responseHead;
        workstationComputeOutput.pixels[base+1u]=vec4<f32>(responseDropped,f32(responseLeaves));
        for(var i:u32=0u;i<8u;i++){{
            workstationComputeOutput.pixels[base+2u+i*3u]=vec4<f32>(responsePositions[i],0.0);
            workstationComputeOutput.pixels[base+3u+i*3u]=vec4<f32>(responseDirections[i],0.0);
            workstationComputeOutput.pixels[base+4u+i*3u]=vec4<f32>(responseWeights[i],0.0);
        }}
        for(var i:u32=0u;i<4u;i++){{workstationComputeOutput.pixels[base+26u+i]=vec4<f32>(responseBases[i],0.0);}}
    }}
}}
'''
    generated = original + '\n' + support + '\n' + trace + '\n' + bake
    frame = bytearray(data)
    struct.pack_into('<2f', frame, 8*16, float(args.edge), float(args.edge))
    meta = {'schema': 1, 'kind': 'baked_ruby_optical_response', 'edge': args.edge,
        'rows_per_pixel': PIXEL_ROWS, 'rows_per_sample': SAMPLE_ROWS, 'terminal_lobes': LOBES,
        'bounds': bounds, 'training_frame': str(args.frame.resolve()), 'training_sha256': sha(data),
        'source_shader_sha256': sha(original.encode()), 'bake_shader_sha256': sha(generated.encode()),
        'bake_frame_sha256': sha(frame), 'scale': pose[3], 'training_pose': pose,
        'glyph_footprint_local': footprint,
        'layout': 'pixel-major top-to-bottom, sample-major; head, dropped RGB/leaves, eight origin/direction/weight triplets, four engraving bases',
        'limits': 'One ruby, fixed orthographic view/shape/material/scale/glyph. Top-eight exterior lobes and filtered spatial response are approximations. Engraving relighting is exact for light colors/intensities; light movement uses a geometric-gain ratio and freezes internal escape response.',
        'installed': False}
    output_paths(args)
    args.output.with_suffix('.wgsl').write_text(generated)
    args.output.with_suffix('.bin').write_bytes(frame)
    args.output.with_suffix('.json').write_text(json.dumps(meta, indent=2)+'\n')
    print(json.dumps({'metadata':str(args.output.with_suffix('.json')), 'edge':args.edge, 'rows_per_pixel':PIXEL_ROWS}))


def evaluator(meta, base, training):
    edge = meta['edge']
    bounds = meta['bounds']
    # Preserve the independent runtime basis for each live light. Translation
    # changes its geometric gain; its in-glass directional attenuation is baked.
    gains = []
    for light in range(3):
        at = 16+light*5
        position = row(training, at)[:3]
        normal = row(training, at+3)[:3]
        delta = [position[i]-meta['training_pose'][i] for i in range(3)]
        distance = math.sqrt(sum(v*v for v in delta))
        cosine = max(0., sum(normal[i]*(-delta[i]/distance) for i in range(3)))
        gains.append(max(cosine/(distance*distance), 1e-12))
    return WGSL_FLOOR + f'''
// Runtime lookup: no gemTrace, gemNearest, gemControlExit or gemAny call.
var<private> responseAccepted:i32=0;
fn responseEnvironment(origin:vec3<f32>,direction:vec3<f32>)->vec3<f32> {{
    let env=gemEnvironment(origin,direction);
    if(env.x<0.0){{return responseFloor(origin+direction*env.w,-direction);}}
    return env.xyz;
}}
fn responseTexel(cell:vec2<i32>,sampleIndex:i32,origin:vec3<f32>)->vec3<f32> {{
    let p=clamp(cell,vec2<i32>(0),vec2<i32>({edge-1}));
    let at={base}+(p.y*{edge}+p.x)*60+sampleIndex*30;
    let head=gemRow(at);let pose=gemRow(192);
    if(head.w<0.5){{return responseEnvironment(origin,vec3<f32>(0.0,0.0,-1.0));}}
    let firstPoint=pose.xyz+head.xyz*pose.w;
    let firstDistance=origin.z-firstPoint.z;
    if(firstDistance<=0.0000001||firstDistance>=gemEnvironment(origin,vec3<f32>(0.0,0.0,-1.0)).w){{
        return responseEnvironment(origin,vec3<f32>(0.0,0.0,-1.0));
    }}
    var result=gemRow(at+26).xyz*gemRow(5).xyz;
    let referenceGain=array<f32,3>({','.join(map(literal,gains))});
    for(var light:i32=0;light<3;light++){{
        let lr=16+light*5;let delta=gemRow(lr).xyz-pose.xyz;
        let distance=max(length(delta),0.000001);
        let gain=max(dot(gemRow(lr+3).xyz,-delta/distance),0.0)/(distance*distance);
        result+=gemRow(at+27+light).xyz*gemRow(lr+4).xyz*(gain/referenceGain[light]);
    }}
    for(var i:i32=0;i<8;i++){{
        let lobe=at+2+i*3;let weight=gemRow(lobe+2).xyz;
        if(gemMax(weight)<=0.0){{continue;}}
        let exitOrigin=pose.xyz+gemRow(lobe).xyz*pose.w;
        result+=weight*responseEnvironment(exitOrigin,gemRow(lobe+1).xyz);
    }}
    return result;
}}
fn responseSample(origin:vec3<f32>,sampleIndex:i32)->vec3<f32> {{
    let xy=(origin.xy-gemRow(192).xy)/gemRow(192).w;
    let uv=vec2<f32>((xy.x-({literal(bounds[0])}))/{literal(bounds[2]-bounds[0])},
                     (({literal(bounds[3])})-xy.y)/{literal(bounds[3]-bounds[1])});
    if(any(uv<vec2<f32>(0.0))||any(uv>vec2<f32>(1.0))){{return responseEnvironment(origin,vec3<f32>(0.0,0.0,-1.0));}}
    let location=uv*f32({edge})-vec2<f32>(0.5);let cell=vec2<i32>(floor(location));let f=fract(location);
    let a=responseTexel(cell,sampleIndex,origin);
    let b=responseTexel(cell+vec2<i32>(1,0),sampleIndex,origin);
    let c=responseTexel(cell+vec2<i32>(0,1),sampleIndex,origin);
    let d=responseTexel(cell+vec2<i32>(1,1),sampleIndex,origin);
    responseAccepted+=1;
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}}
fn workstationShade(screen:vec2<f32>)->vec4<f32> {{
    responseAccepted=0;
    var result=vec3<f32>(0.0);
    for(var sampleIndex:i32=0;sampleIndex<2;sampleIndex++){{
        let xi=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sampleIndex));
        let pixel=screen+(xi-vec2<f32>(0.5))*2.0/gemRow(8).xy;
        let origin=gemRow(1).xyz+gemRow(2).xyz*(pixel.x*gemRow(2).w)+gemRow(3).xyz*(pixel.y*gemRow(3).w);
        result+=responseSample(origin,sampleIndex);
    }}
    return vec4<f32>(gemDisplay(result*0.5),f32(responseAccepted));
}}
'''


def evaluate(args):
    meta = json.loads(args.metadata.read_text())
    require(meta['schema']==1 and meta['edge'] in (128,256) and meta['rows_per_pixel']==PIXEL_ROWS,
            'Unsupported response layout')
    training = Path(meta['training_frame']).read_bytes()
    require(sha(training)==meta['training_sha256'], 'Training packet changed')
    frame = args.frame.read_bytes()
    validate_fixture(frame)
    require(len(frame)==len(training), 'Fixture layout changed')
    normalized = bytearray(frame)
    # Allow only translations, bounds, ambient/light colors and light centers.
    for address in (5,13,14,16,20,21,25,26,30,192):
        normalized[address*16:address*16+12] = training[address*16:address*16+12]
    require(normalized==training, 'Fixture changed baked geometry/material/glyph/camera assumptions')
    atlas = args.atlas.read_bytes()
    require(len(atlas)==meta['edge']**2*PIXEL_ROWS*16 and len(frame)+len(atlas)<=MAX_BYTES,
            'Atlas size or combined packet budget invalid')
    values = array.array('f')
    values.frombytes(atlas)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(v) for v in values), 'Nonfinite response coefficients')
    original = args.shader.read_text()
    require(sha(original.encode())==meta['source_shader_sha256'], 'Canonical shader changed')
    start, end, _ = function(original, 'workstationShade')
    generated = original[:start]+evaluator(meta,len(frame)//16,training)+original[end:]
    output_paths(args)
    args.output.with_suffix('.wgsl').write_text(generated)
    args.output.with_suffix('.bin').write_bytes(frame+atlas)
    report = {'kind':'baked_ruby_response_lookup','atlas_sha256':sha(atlas),'frame_sha256':sha(frame),
              'shader_sha256':sha(generated.encode()),'atlas_bytes':len(atlas),
              'alpha':'Number of primary samples inside the response rectangle (includes silhouette/background texels).',
              'limits':meta['limits'],'installed':False}
    args.output.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    commands=parser.add_subparsers(dest='command',required=True)
    for name in ('prepare','evaluate'):
        p=commands.add_parser(name)
        p.add_argument('--shader',type=Path,required=True)
        p.add_argument('--frame',type=Path,required=True)
        p.add_argument('--output',type=Path,required=True)
        if name=='prepare':p.add_argument('--edge',type=int,default=256)
        else:
            p.add_argument('--atlas',type=Path,required=True)
            p.add_argument('--metadata',type=Path,required=True)
    args=parser.parse_args()
    (prepare if args.command=='prepare' else evaluate)(args)


if __name__=='__main__':main()
