#!/usr/bin/env python3
"""Measure separate fast/fallback kernels against the same cached renderer.

This is a headless architecture experiment, not a native app build or promotion.
It must match the reference image, which still has the known coarse lettering.
"""
from pathlib import Path
import argparse
import array
import datetime
import hashlib
import json
import math
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
import ruby_split


def compare(reference, candidate):
    images = []
    for path in [reference, candidate]:
        values = array.array('f')
        values.frombytes(path.read_bytes())
        if sys.byteorder != 'little': values.byteswap()
        if len(values) != 640*480*4 or not all(math.isfinite(v) for v in values):
            raise RuntimeError('Incomplete or nonfinite output: '+str(path))
        images.append(values)
    differences = [abs(a-b) for a,b in zip(*images)]
    rgb = [v for i,v in enumerate(differences) if i%4 != 3]
    return {'max_rgb_error':max(rgb),'rgb_rmse':math.sqrt(sum(v*v for v in rgb)/len(rgb)),
        'rgb_channels_over_0_00003':sum(v>0.00003 for v in rgb),
        'alpha_count_mismatches':sum(differences[i]!=0. for i in range(3,len(differences),4))}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference',type=Path)
    reference=parser.parse_args().reference.resolve()
    original=json.loads((reference/'report.json').read_text())
    if original['status'] != 'validated_pending_visual_and_performance_review':
        raise RuntimeError('Requires a completed reference experiment')
    fixture=Path(original['plan']['frame'])
    frame_hash=hashlib.file_digest(fixture.open('rb'),'sha256').hexdigest()
    out=ROOT/'output/checks'/('ruby-split-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    report={'status':'running','native_integration':False,'installed':False,
        'reference':str(reference),'caveat':'Reference retains known coarse panel lettering', 'steps':[]}
    def save():
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    def run(name,command,timeout=180):
        print(name,flush=True)
        log=out/(name+'.log')
        try:
            run_checked(list(map(str,command)),cwd=ROOT,log=log,timeout=timeout)
        except BaseException:
            print(log.read_text(errors='replace')[-7000:],flush=True)
            raise
        report['steps'].append(name);save()
    try:
        shader=out/'split.wgsl'
        shader.write_text(ruby_split.source((reference/'response.wgsl').read_text()))
        probe=out/'ruby-split-probe'
        run('compile-split-probe',['xcrun','clang++','-std=c++17','-O2','-I/opt/homebrew/include',
            'BuildSupport/Workstation/ruby_split_probe.cpp','-L/opt/homebrew/lib','-lwgpu_native',
            '-Wl,-rpath,/opt/homebrew/lib','-o',probe])
        cases=[('reference',reference/'ruby-cache-probe',reference/'response.wgsl','warm'),
            ('split',probe,shader,'warm'),('split-cold',probe,shader,'cold'),
            ('reference-repeat',reference/'ruby-cache-probe',reference/'response.wgsl','warm')]
        for name,executable,source,mode in cases:
            run('gpu-'+name,[executable,source,fixture,reference/'response.plan.bin',640,480,out/name,6,mode])
            report[name]=json.loads((out/(name+'.json')).read_text())
            if report[name].get('nonfinite_output_channels',0):
                raise RuntimeError('Nonfinite split output: '+name)
        for name in ['split','split-cold']:
            diff=compare(out/'reference.rgba32f',out/(name+'.rgba32f'))
            report[name+'-image-comparison']=diff
            save()
            if diff['max_rgb_error']>0.00003 or diff['alpha_count_mismatches']:
                raise RuntimeError('Split rendering changed reference output: '+name)
        for name in ['reference','split']:
            run('image-'+name,['/usr/bin/sips','-s','format','png',out/(name+'.ppm'),'--out',out/(name+'.png')])
        before=report['reference']['median_gpu_ms']
        report['warm_speedup']=before/report['split']['median_gpu_ms']
        report['cold_speedup']=before/report['split-cold']['median_gpu_ms']
        report['baseline_drift_percent']=100*(report['reference-repeat']['median_gpu_ms']/before-1)
        if frame_hash != hashlib.file_digest(fixture.open('rb'),'sha256').hexdigest():
            raise RuntimeError('Reference scene changed during validation')
        report['status']='completed_headless_only';save()
        print(json.dumps({'report':str(out/'report.json'),'warm_speedup':report['warm_speedup'],
            'cold_speedup':report['cold_speedup'],'image_comparison':report['split-image-comparison']}),flush=True)
    except BaseException as error:
        report['status']='failed';report['error']=str(error);save();raise


if __name__ == '__main__':
    main()
