#!/usr/bin/env python3
"""Compare current monolithic shading with exact separate fast/fallback passes.

Uses completed 640x480 timeline fixtures. Does not build/install the app.
All shading passes and inter-pass gaps are included in each reported GPU time.
"""
from pathlib import Path
import datetime
import json
import os
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT/'BuildSupport'))
from checked_process import run_checked
from probe_engraving_factors import digest, compare
import ruby_current_split

FIXTURE = ROOT/'output/checks/ruby-timeline-native-20261031-224550-202605'
VALIDATOR = ROOT/'output/checks/engraving-factors-20261031-230555-267487/validate'


def main():
    out = ROOT/'output/checks'/('current-split-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    inputs = [FIXTURE/name for name in ('report.json','timeline-probe','baseline.wgsl',
        'candidate.wgsl','baseline.frame.bin','candidate.frame.bin','baseline.plan.bin','candidate.plan.bin')]
    inputs += [Path(__file__).resolve(),Path(ruby_current_split.__file__),
        ROOT/'BuildSupport/Workstation/ruby_current_split_probe.cpp',
        ROOT/'BuildSupport/Workstation/probe_engraving_factors.py',VALIDATOR,
        ROOT/'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine']
    before = {str(p):digest(p) for p in inputs}
    report = {'status':'running','installed':False,'native_fps_measured':False,
        'scope':'Same immutable scene, two sample locations, optics, sharp text and response cache. '
                'Separate fast pass has no general ray queue in its reachable functions. Any pixel '
                'with a rejected sample is recomputed by the unchanged complete shader. '
                'GPU total includes both shading passes and their gap; cold includes all bakes/resolves. '
                'The older full-occlusion split was slower; this tests the different current renderer.',
        'input_sha256':before,'steps':[],'cases':{}}

    def save():
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')

    def run(label, argv, timeout=300):
        print(label,flush=True)
        log = out/(label+'.log')
        env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
        try:
            run_checked(list(map(str,argv)),cwd=ROOT,log=log,env=env,timeout=timeout)
        except BaseException:
            if log.exists(): print(log.read_text(errors='replace')[-4500:],flush=True)
            raise
        report['steps'].append(label)
        save()

    try:
        if shutil.disk_usage(ROOT).free < 512*1024**2:
            raise RuntimeError('Need512MiB for finite split diagnostic')
        reference = json.loads((FIXTURE/'report.json').read_text())
        if reference.get('status')!='completed_staged_diagnostics':
            raise RuntimeError('Reference native fixture is incomplete')
        run('test-transform',[sys.executable,ROOT/'BuildSupport/Workstation/test_ruby_current_split.py'])
        probe = out/'current-split-probe'
        run('compile-split-probe',['xcrun','clang++','-std=c++17','-O2',
            '-I/opt/homebrew/include',ROOT/'BuildSupport/Workstation/ruby_current_split_probe.cpp',
            '-L/opt/homebrew/lib','-lwgpu_native','-Wl,-rpath,/opt/homebrew/lib','-o',probe])
        for family in ('baseline','candidate'):
            shader = out/(family+'-split.wgsl')
            shader.write_text(ruby_current_split.transform((FIXTURE/(family+'.wgsl')).read_text()))
            run('validate-'+family,[VALIDATOR,shader])
            cases = [('warm-reference-a',False,'warm','resolve-once'),
                     ('warm-split-a',True,'warm','resolve-once'),
                     ('warm-split-b',True,'warm','resolve-once'),
                     ('warm-reference-b',False,'warm','resolve-once')]
            if family=='candidate':
                cases += [('cold-reference',False,'cold','resolve'),('cold-split',True,'cold','resolve')]
            for case, split, mode, resolve in cases:
                label=family+'-'+case
                source=shader if split else FIXTURE/(family+'.wgsl')
                run('gpu-'+label,[probe if split else FIXTURE/'timeline-probe',source,
                    FIXTURE/(family+'.frame.bin'),FIXTURE/(family+'.plan.bin'),640,480,
                    out/label,8,mode,resolve])
                measured=json.loads((out/(label+'.json')).read_text())
                if measured.get('status')!='completed' or measured.get('nonfinite_output_channels')!=0:
                    raise RuntimeError('Invalid GPU result '+label)
                appearance=compare(out/(family+'-warm-reference-a.rgba32f'),out/(label+'.rgba32f'))
                report['cases'][label]={'probe':measured,'appearance':appearance}
                save()
                if appearance['channels_over_3e_minus5'] or appearance['alpha_channels_changed']:
                    raise RuntimeError('Split changed reference pixels/cache coverage '+label)
        report['status']='completed_current_split_diagnostic'
    except BaseException as error:
        report.update(status='failed_current_split_diagnostic',error=str(error))
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
