#!/usr/bin/env python3
"""Private evaluator of certified affine ruby paths; never installs app assets.

The table selects continuous path formulas, not image colors. Geometry, IOR,
roughness and scale are fixed by the compiler; translation and studio lighting
remain live. This first experiment requires one isolated flat-faceted ruby. It is not
a whole-workstation renderer and must not be installed as one.
"""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import struct
import sys


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def rows(data):
    require(len(data) % 16 == 0, 'Incomplete float4 packet')
    values = array.array('f')
    values.frombytes(data)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(v) for v in values), 'Nonfinite packet')
    return values


def literal(value):
    value = float(value)
    require(math.isfinite(value), 'Nonfinite shader constant')
    result = format(value, '.9g')
    return result if '.' in result or 'e' in result.lower() else result + '.0'


def replace_once(source, old, new):
    require(source.count(old) == 1, 'Canonical shader marker changed: ' + old)
    return source.replace(old, new)


def kernel(meta, base):
    grid = int(meta['grid_size'])
    bounds = [literal(v) for v in meta['bounds']]
    pose = int(meta.get('pose_row', 192))
    material = int(meta['material_row_base'])
    medium = int(meta.get('medium', 4))
    epsilon = literal(meta['epsilon_world'])
    mapping = base + int(meta['map_base'])
    programs = base + int(meta['program_base'])
    # firstStepRow values in the program table are relative to its binary.
    return f'''
// PRIVATE COMPILED OPTICAL PATH EXPERIMENT. No cached image pixels.
// A certified cell selects affine geometry and constant optical factors.
// Uncertified cells execute the complete canonical integrator.
var<private> compiledGemAccepted:u32=0u;
fn compiledGemAffine(row:i32, xy:vec2<f32>)->f32 {{
    let coefficient=gemRow(row);
    return coefficient.x*xy.x+coefficient.y*xy.y+coefficient.z;
}}
fn compiledGemExternal(origin:vec3<f32>, direction:vec3<f32>)->vec3<f32> {{
    // The generator checks every exterior branch against all original triangles
    // over its accepted cell. Floor and finite area lights remain live.
    let environment=gemEnvironment(origin,direction);
    if(environment.x<0.0){{return gemFloor(origin+direction*environment.w,-direction);}}
    return environment.xyz;
}}
fn compiledGemTrace(origin:vec3<f32>, direction:vec3<f32>, random:vec2<f32>, sampleIndex:i32)->vec3<f32> {{
    let pose=gemRow({pose});
    let xy=(origin.xy-pose.xy)/pose.w;
    let uv=(xy-vec2<f32>({bounds[0]},{bounds[1]}))/vec2<f32>({literal(float(meta['bounds'][2])-float(meta['bounds'][0]))},{literal(float(meta['bounds'][3])-float(meta['bounds'][1]))});
    if(any(uv<vec2<f32>(0.0))||any(uv>=vec2<f32>(1.0))||sampleIndex<0||sampleIndex>=2){{
        return gemTrace(origin,direction,random);
    }}
    let cell=vec2<i32>(floor(uv*f32({grid})));
    let index=sampleIndex*{grid*grid}+cell.y*{grid}+cell.x;
    let programID=i32(gemRow({mapping}+index/4)[index%4]);
    if(programID<=0||programID>={int(meta['program_count'])}){{return gemTrace(origin,direction,random);}}
    let program=gemRow({programs}+programID);
    let count=i32(program.y);
    if(count<=0||count>16){{return gemTrace(origin,direction,random);}}
    let firstAt={base}+i32(program.x);
    let firstLocal=vec3<f32>(compiledGemAffine(firstAt,xy),compiledGemAffine(firstAt+1,xy),compiledGemAffine(firstAt+2,xy));
    let firstPoint=pose.xyz+firstLocal*pose.w;
    let firstDistance=dot(firstPoint-origin,direction);
    // A moved finite light card can be in front of the object itself.
    if(firstDistance<=0.0000001||firstDistance>=gemEnvironment(origin,direction).w){{
        return gemTrace(origin,direction,random);
    }}
    var result=vec3<f32>(0.0);
    var weight=vec3<f32>(1.0);
    var currentOrigin=origin;
    let sigma=gemRow({material+1}).xyz;
    for(var event:i32=0;event<16;event++){{
        if(event>=count||f32(event)>=gemRow(8).w){{break;}}
        let at={base}+i32(program.x)+event*10;
        let inside=gemRow(at+3).w>0.5;
        let incident=gemRow(at+4).xyz;
        let localPoint=vec3<f32>(compiledGemAffine(at,xy),compiledGemAffine(at+1,xy),compiledGemAffine(at+2,xy));
        let point=pose.xyz+localPoint*pose.w;
        let distance=compiledGemAffine(at+3,xy);
        if(inside){{
            let engraving=gemControlEngraving(currentOrigin,incident,{medium},distance*pose.w);
            result+=weight*engraving.xyz;
            weight*=engraving.w;
            weight*=exp(-sigma*distance);
        }}
        let reflection=weight*gemRow(at+8).xyz;
        let transmission=weight*gemRow(at+9).xyz;
        let reflected=gemRow(at+6).xyz;
        let transmitted=gemRow(at+7).xyz;
        let depth=event+1;
        if(f32(depth)>=gemRow(8).z){{break;}}
        var externalWeight=reflection;
        var externalDirection=reflected;
        var internalWeight=transmission;
        var internalDirection=transmitted;
        if(inside){{
            externalWeight=transmission;externalDirection=transmitted;
            internalWeight=reflection;internalDirection=reflected;
        }}
        if(gemMax(externalWeight)>0.00005){{
            result+=externalWeight*compiledGemExternal(point+externalDirection*{epsilon},externalDirection);
        }}
        if(gemMax(internalWeight)<=0.00005){{break;}}
        weight=internalWeight;
        currentOrigin=point+internalDirection*{epsilon};
    }}
    compiledGemAccepted+=1u;
    return result;
}}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--shader', type=Path, required=True)
    parser.add_argument('--frame', type=Path, required=True)
    parser.add_argument('--program', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    meta = json.loads(args.program.read_text())
    require(meta['sample_count'] == 2 and meta['expected_depth'] == 16 and meta['expected_events'] == 64,
            'Unsupported optical program budgets')
    require(1 <= meta['program_count'] <= 100_000 and meta['grid_size'] in (16, 32, 64, 128, 256),
            'Unbounded program dictionary')
    table_path = args.program.with_suffix('.bin')
    table = table_path.read_bytes()
    require(0 < len(table) <= 32 * 1024 * 1024, 'Program table exceeds 32 MiB bound')
    table_values = rows(table)
    require(digest(table) == meta['packet_sha256'], 'Compiled program binary changed')
    frame = args.frame.read_bytes()
    training = Path(meta['source_frame']).read_bytes()
    require(digest(training) == meta['source_frame_sha256'], 'Training optical packet changed')
    require(len(frame) == len(training), 'Held-out optical packet has a different layout')
    # Only these live transformations are supported by this experiment. Freeze
    # every other byte, including geometry, camera, material and density tables.
    normalized = bytearray(frame)
    for address in (13, 14, 16, 21, 26, 192):
        begin = address * 16
        normalized[begin:begin+12] = training[begin:begin+12]
    require(normalized == training, 'Held-out frame changed a compiled optical assumption')
    del normalized, training
    frame_values = rows(frame)
    row = lambda index: list(frame_values[index*4:index*4+4])
    require(len(frame) >= 49408 * 16 and row(0)[1:3] == [0., 2.]
            and row(8)[2:] == [16., 64.] and row(12)[0] == 1., 'Fixture must retain one isolated ruby and original quality budgets')
    require(row(1)[3] == 0. and row(2)[:3] == [1., 0., 0.]
            and row(3)[:3] == [0., 1., 0.] and row(4)[:3] == [0., 0., -1.],
            'Compiled paths require the original fixed orthographic view')
    require(row(192)[3] == meta['scale'] and meta.get('pose_row', 192) == 192,
            'This experiment fixes the first ruby shape and scale')
    require(all(row(index)[0] == 0. for index in (234, 240, 245, 248))
            and row(245)[2] == 0. and row(250)[3] == 0. and row(252)[3] == 0.
            and row(232)[1:3] == [0., 0.] and row(247)[1] == 0.,
            'External geometry invalidates isolated-object optical paths')
    for index, expected in meta.get('frozen_rows', {}).items():
        require(row(int(index)) == expected, 'Frozen optical parameter changed: row ' + index)
    total_rows = len(table_values) // 4
    grid_cells = 2 * meta['grid_size'] ** 2
    require(0 <= meta['map_base'] and meta['map_base'] + (grid_cells+3)//4 <= total_rows,
            'Map outside program binary')
    require(0 <= meta['program_base'] and meta['program_base'] + meta['program_count'] <= total_rows,
            'Program directory outside binary')
    for program in range(1, meta['program_count']):
        start = 4 * (meta['program_base'] + program)
        offset, count = table_values[start:start+2]
        require(offset == int(offset) and count == int(count) and 1 <= count <= 16
                and meta['step_base'] <= offset and offset + count*10 <= total_rows,
                'Invalid program range')
    map_start = meta['map_base'] * 4
    require(all(v == int(v) and 0 <= v < meta['program_count']
                for v in table_values[map_start:map_start+grid_cells]), 'Invalid program map')
    base = len(frame) // 16
    require(len(frame) + len(table) <= 128 * 1024 * 1024 and base + total_rows < 16_777_216,
            'Combined optical storage exceeds private probe limits')
    original = args.shader.read_text()
    marker = 'fn workstationShade(screen:vec2<f32>)->vec4<f32> {'
    generated = replace_once(original, marker, kernel(meta, base) + '\n' + marker + '\n    compiledGemAccepted=0u;')
    generated = replace_once(generated, 'result+=gemTrace(origin,direction,random);',
                             'result+=compiledGemTrace(origin,direction,random,sampleIndex);')
    generated = replace_once(generated, 'return vec4<f32>(gemDisplay(result/samples),1.0);',
                             'return vec4<f32>(gemDisplay(result/samples),f32(compiledGemAccepted));')
    production = Path(__file__).resolve().parents[2] / 'Procedures/Shaders'
    for suffix in ('.wgsl', '.bin', '.json'):
        target = args.output.with_suffix(suffix).resolve()
        require(not target.is_relative_to(production) and target not in
                (args.shader.resolve(), args.frame.resolve(), args.program.resolve(), table_path.resolve()),
                'Private output must not overwrite source, program or production files')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.with_suffix('.wgsl').write_text(generated)
    args.output.with_suffix('.bin').write_bytes(frame + table)
    report = {'kind': 'private_compiled_optical_paths', 'program': str(args.program.resolve()),
              'frame_sha256': digest(frame), 'table_sha256': digest(table),
              'source_shader_sha256': digest(original.encode()), 'shader_sha256': digest(generated.encode()),
              'program_rows': total_rows, 'original_rows': base, 'table_bytes': len(table),
              'alpha_diagnostic': 'Number of primary samples evaluated with compiled paths; RGB remains display color.',
              'limits': 'One isolated flat-faceted ruby with per-cell exterior self-miss checks, fixed orthographic view/scale/IOR/roughness. Live translation, engraving and lights. Not a whole-workstation FPS result.',
              'production_installed': False}
    args.output.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'shader': str(args.output.with_suffix('.wgsl')),
                      'frame': str(args.output.with_suffix('.bin')), 'table_bytes': len(table)}))


if __name__ == '__main__':
    main()
