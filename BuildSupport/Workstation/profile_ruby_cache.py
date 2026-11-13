#!/usr/bin/env python3
"""Finite cost isolation, using a previously validated probe and captured scene.

These deliberately incomplete shader variants are diagnostics, never release
candidates. Timings are not additive: compiler resource use and paths change.
"""
from pathlib import Path
import argparse
import datetime
import json
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked


def body(source, name, replacement):
    marker = 'fn ' + name + '('
    assert source.count(marker) == 1, marker
    start = source.index('{', source.index(marker))
    end, depth = start + 1, 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[:start+1] + '\n' + replacement + '\n' + source[end-1:]


def once(source, old, new):
    assert source.count(old) == 1, old
    return source.replace(old, new, 1)


DIRECT_ENVIRONMENT = '''
    let environment=gemEnvironment(origin,ray);
    var radiance=environment.xyz;
    if(radiance.x<0.0){radiance=gemFloor(origin+ray*environment.w,-ray);}
    return vec4<f32>(radiance,1.0);
'''

# Upper-bound experiment for a separate cached-pixel pipeline. Missing entries,
# opaque primary hits and nonterminal exterior rays are intentionally omitted.
SMALL_KERNEL = '''
    let environment=gemEnvironment(origin,direction);
    let hit=gemNearest(origin,direction,-1,environment.w);
    if(hit.panel<0||hit.distance<=0.0){
        var radiance=environment.xyz;
        if(radiance.x<0.0){radiance=gemFloor(origin+direction*environment.w,-direction);}
        return radiance;
    }
    let index=rubyResponseAddress(origin,direction,hit.panel,random);
    if(index<0){return vec3<f32>(0.0);}
    rubyUsedSamples+=1u;
    let address=u32(index);
    let slot=rubyResponseSlot(hit.panel);
    let pose=rubyPlan.rows[2u+u32(slot)*8u];
    var result=rubyResponse.rows[address+26u].xyz;
    for(var lobe:u32=0u;lobe<8u;lobe++){
        let leaf=address+2u+3u*lobe;
        let weight=rubyResponse.rows[leaf+2u].xyz;
        if(gemMax(weight)<=0.0){continue;}
        let position=rubyResponse.rows[leaf];
        let exitOrigin=pose.xyz+position.xyz*pose.w;
        let exitDirection=gemSafeUnit(rubyResponse.rows[leaf+1u].xyz);
        let terminal=gemTerminal(exitOrigin,exitDirection);
        if(terminal.w>0.5){result+=weight*terminal.xyz;}
    }
    return result;
'''


def variants(source):
    return {
        'warm-reference': (source, 'Unchanged accepted-coverage implementation'),
        'omit-floor-lighting': (body(source,'gemFloor','return gemRow(6).xyz;'),
            'Constant floor color removes floor lighting math; not visually correct'),
        'omit-exterior-occlusion': (body(source,'gemTerminal',DIRECT_ENVIRONMENT),
            'Exterior rays see studio directly, omitting other objects'),
        'omit-cached-leaf-evaluation': (once(source,
            'let terminal=gemTerminal(exitOrigin,exitDirection);',
            'let terminal=vec4<f32>(0.0,0.0,0.0,1.0);'),
            'Cached paths emit no exterior radiance; other primary paths stay canonical'),
        'omit-cached-continuations': (once(source,
            '''else if(count<16){
                        pending[count]=GemRay(exitOrigin,-1,exitDirection,i32(position.w),weight,priority);count++;
                    }''', ''), 'Keep cached terminal evaluation but omit paths that encounter other geometry'),
        'small-cached-kernel': (body(source,'gemTrace',SMALL_KERNEL),
            'No full ray queue; preserves cached terminal paths and directly visible environment, omits all fallback work'),
        'warm-repeat': (source, 'Repeat unchanged implementation to measure drift'),
    }


def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument('reference', type=Path)
    reference = args.parse_args().reference.resolve()
    previous = json.loads((reference/'report.json').read_text())
    if previous['status'] != 'validated_pending_visual_and_performance_review':
        raise RuntimeError('Requires a completed, validated reference experiment')
    source = (reference/'response.wgsl').read_text()
    fixture = Path(previous['plan']['frame'])
    probe = reference/'ruby-cache-probe'
    plan = reference/'response.plan.bin'
    out = ROOT/'output/checks'/('ruby-cost-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    report = {'status':'running', 'reference':str(reference), 'production_candidate':False,
              'interpretation':'Diagnostic omissions isolate cost; differences are not additive.', 'cases':{}}
    def save():
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')

    try:
        for name,(shader,description) in variants(source).items():
            shader_path=out/(name+'.wgsl')
            shader_path.write_text(shader)
            print(name,flush=True)
            log=out/(name+'.log')
            try:
                run_checked([str(probe),str(shader_path),str(fixture),str(plan),'640','480',
                    str(out/name),'6','warm'],cwd=ROOT,log=log,timeout=180)
            except BaseException:
                print(log.read_text(errors='replace')[-7000:],flush=True)
                raise
            result=json.loads((out/(name+'.json')).read_text())
            if result.get('nonfinite_output_channels',0):
                raise RuntimeError('Nonfinite diagnostic output: '+name)
            report['cases'][name]={'description':description,'median_gpu_ms':result['median_gpu_ms'],
                'startup_bake_gpu_ms':result['startup_bake_gpu_ms']}
            save()
        report['status']='completed_diagnostic_only'
        save()
        print(json.dumps({'report':str(out/'report.json'),'cases':report['cases']}),flush=True)
    except BaseException as error:
        report['status']='failed';report['error']=str(error);save();raise


if __name__ == '__main__':
    main()
