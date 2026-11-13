#!/usr/bin/env python3
"""One bounded primary-visibility diagnostic, with no production shader edits.

Separate timings of full shading and query-only kernels are NOT additive costs.
Counter variants are coverage evidence only, never optimization candidates.
Uses the completed, immutable 640x480 timeline fixture; no native FPS claim.
"""
from pathlib import Path
import array
import datetime
import hashlib
import json
import math
import os
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'BuildSupport'))
from checked_process import run_checked

FIXTURE = ROOT/'output/checks/ruby-timeline-native-20261031-224550-202605'
VALIDATOR = ROOT/'output/checks/engraving-factors-20261031-230555-267487/validate'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def once(source, old, new):
    if source.count(old) != 1:
        raise ValueError('Primary diagnostic marker changed: '+old[:100])
    return source.replace(old, new, 1)


def function_span(source, name):
    marker = 'fn '+name+'('
    if source.count(marker) != 1:
        raise ValueError('Function identity changed: '+name)
    start = source.index(marker)
    end = source.index('{', start)+1
    depth = 1
    while depth and end < len(source):
        depth += (source[end] == '{')-(source[end] == '}')
        end += 1
    if depth:
        raise ValueError('Unbalanced function: '+name)
    return start, end


def edit_function(source, name, edits):
    start, end = function_span(source, name)
    body = source[start:end]
    for old, new in edits:
        body = once(body, old, new)
    return source[:start]+body+source[end:]


COUNTERS = {
    'control-visits': [('gemControlHit', 'var instanceLo:vec4<f32>=gemRow(instanceNode);', 0),
        ('gemControlHit', 'var local:vec3<f32>=(origin-pose.xyz)/pose.w;', 1),
        ('gemControlHit', 'var lo:vec4<f32>=gemRow(node);', 2)],
    'timeline-visits': [('gemTimelineHit', 'var instance:i32=count-1-candidate;', 0),
        ('gemTimelineMesh', 'var shifted:vec3<f32>=local+vec3<f32>(phase-f32(tile)*0.22,0.0,0.0);', 1),
        ('gemTimelineMesh', 'var low:vec4<f32>=gemRow(node);', 2)],
    'primitive-visits': [('gemControlHit', 'var at:i32=i32(hi.w)+triangle*6;', 0),
        ('gemTimelineMesh', 'var source:i32=i32(high.w)+triangle*6;', 1),
        ('gemEnterPanel', 'var plane:vec4<f32>=gemRow(base+6+i);', 2)],
}
COUNTER_NAMES = {
    'control-visits': ['control_instance_bvh_nodes', 'control_mesh_candidates', 'control_mesh_bvh_nodes'],
    'timeline-visits': ['timeline_instance_candidates', 'timeline_tiles', 'timeline_mesh_bvh_nodes'],
    'primitive-visits': ['control_triangles', 'timeline_triangles', 'panel_planes'],
}


def transform(source, variant):
    if variant == 'full':
        return source
    if variant not in ('primary', *COUNTERS):
        raise ValueError('Unknown primary diagnostic')
    if len(source.encode()) > 2*1024**2 or '// PRIMARY_VISIBILITY_DIAGNOSTIC' in source:
        raise ValueError('Repeated or oversized primary diagnostic')
    # Keep the original camera, two subpixel offsets, ray normalization and
    # sample order. Only replace the traced color with a query signature.
    source = edit_function(source, 'workstationShade', [
        ('    rubyTimelineUsedSamples=0u;', '    rubyTimelineUsedSamples=0u;\n    primaryVisits=vec3<u32>(0u);'),
        ('result+=gemTrace(origin,direction,random);', 'result+=primaryVisibilitySignature(origin,direction);'),
        ('return vec4<f32>(gemDisplay(result/samples),f32(rubyTimelineUsedSamples));',
         'return vec4<f32>(result/samples,0.0);' if variant == 'primary'
         else 'return vec4<f32>(vec3<f32>(primaryVisits),0.0);')])
    source += '''
fn primaryVisibilitySignature(origin:vec3<f32>,direction:vec3<f32>)->vec3<f32> {
    let environment=gemEnvironment(origin,direction);
    let hit=gemNearest(origin,direction,-1,environment.w);
    // Consume every hit/environment field so the uninstrumented query cannot
    // become a boolean occlusion-only query. This is diagnostic data, not color.
    return hit.normal+environment.xyz*0.000001+
        vec3<f32>(f32(hit.panel)*0.000001,hit.distance*1e-20,environment.w*1e-20);
}
'''
    for function, marker, channel in COUNTERS.get(variant, []):
        source = edit_function(source, function, [(marker,
            'primaryVisits['+str(channel)+']+=1u; '+marker)])
    return '// PRIMARY_VISIBILITY_DIAGNOSTIC '+variant+'\nvar<private> primaryVisits:vec3<u32>;\n'+source


def visit_summary(path, names):
    values = array.array('f')
    values.frombytes(path.read_bytes())
    if sys.byteorder != 'little':
        values.byteswap()
    if len(values) != 640*480*4 or not all(map(math.isfinite, values)):
        raise RuntimeError('Incomplete/nonfinite counter image')
    result = {}
    for channel, name in enumerate(names):
        counts = sorted(values[channel::4])
        if any(value < 0 or value != int(value) for value in counts):
            raise RuntimeError('Noninteger visit count')
        total = sum(map(int, counts))
        result[name] = {'total_two_sample_frame':total,
            'mean_per_primary_sample':total/(2*640*480),
            'pixel_p50':counts[len(counts)//2], 'pixel_p95':counts[math.ceil(.95*len(counts))-1],
            'pixel_max':counts[-1], 'pixels_with_visits':sum(value>0 for value in counts)}
    return result


def main():
    out = ROOT/'output/checks'/('primary-visibility-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    installed = ROOT/'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine'
    inputs = [FIXTURE/name for name in ('report.json','candidate.wgsl','candidate.frame.bin',
                                      'candidate.plan.bin','timeline-probe')]
    current_shader = ROOT/'Procedures/Shaders/workstation.wgsl'
    inputs += [VALIDATOR, installed, Path(__file__).resolve(), current_shader,
               ROOT/'Workstation/RubyResponseShader.rs']
    before = {str(path):digest(path) for path in inputs}
    report = {'status':'running','installed':False,'native_fps_measured':False,
        'scope':'Static authentic 640x480 timeline fixture, identical two primary samples and geometry. '
                'Query-only kernels omit shading and change compiler scheduling/live ranges. Their costs '
                'are isolated, nonadditive evidence, not a precise subtraction from the full kernel. '
                'Counter variants are not used for timing conclusions. No production optimization here.',
        'input_sha256':before,'steps':[],'cases':{}}

    def save():
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')

    def run(label, argv):
        print(label,flush=True)
        log = out/(label+'.log')
        env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
        try:
            run_checked(list(map(str,argv)),cwd=ROOT,log=log,env=env,timeout=300)
        except BaseException:
            if log.exists(): print(log.read_text(errors='replace')[-4000:],flush=True)
            raise
        report['steps'].append(label)
        save()

    try:
        if shutil.disk_usage(ROOT).free < 256*1024**2:
            raise RuntimeError('Need256MiB for finite diagnostics')
        reference = json.loads((FIXTURE/'report.json').read_text())
        if reference.get('status') != 'completed_staged_diagnostics':
            raise RuntimeError('Reference was not completed')
        source = (FIXTURE/'candidate.wgsl').read_text()
        current = current_shader.read_text()
        query_names = ('gemNearest','gemEnterPanel','gemControlHit','gemTimelineHit',
                       'gemTimelineMesh','gemEnvironment')
        report['current_primary_functions_match_fixture'] = all(
            source[slice(*function_span(source,name))] == current[slice(*function_span(current,name))]
            for name in query_names)
        if not report['current_primary_functions_match_fixture']:
            raise RuntimeError('Current primary query source differs from completed fixture')
        for variant in ('full','primary',*COUNTERS):
            shader = out/(variant+'.wgsl')
            shader.write_text(transform(source,variant))
            run('validate-'+variant,[VALIDATOR,shader])
        # Repeats bracket drift; no CPU compilation or native process overlaps.
        for label, variant in [('full-a','full'),('primary-a','primary'),
                               ('primary-b','primary'),('full-b','full'),
                               *((v,v) for v in COUNTERS)]:
            run('gpu-'+label,[FIXTURE/'timeline-probe',out/(variant+'.wgsl'),
                FIXTURE/'candidate.frame.bin',FIXTURE/'candidate.plan.bin',640,480,
                out/label,8 if variant in ('full','primary') else 1,'warm','resolve-once'])
            result = json.loads((out/(label+'.json')).read_text())
            if result.get('status')!='completed' or result.get('nonfinite_output_channels')!=0:
                raise RuntimeError('Invalid GPU diagnostic '+label)
            report['cases'][label] = {'timing_interpretation':'isolated nonadditive timing'
                if variant in ('full','primary') else 'instrumented coverage only; ignore timing',
                'probe':result}
            if variant in COUNTERS:
                report['cases'][label]['visits'] = visit_summary(out/(label+'.rgba32f'),COUNTER_NAMES[variant])
            save()
        report['status']='completed_primary_visibility_diagnostic'
    except BaseException as error:
        report.update(status='failed_primary_visibility_diagnostic',error=str(error))
        raise
    finally:
        changed=[str(p) for p in inputs if not p.is_file() or digest(p)!=before[str(p)]]
        report['changed_inputs']=changed
        if changed: report['status']='failed_identity_check'
        save()
        print(json.dumps({'status':report['status'],'installed':False,'report':str(out/'report.json')}),flush=True)
        if changed: raise RuntimeError('Diagnostic input changed')


if __name__=='__main__':
    main()
