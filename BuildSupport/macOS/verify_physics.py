#!/usr/bin/env python3
"""Mirror-only native verification. Stress failures are retained, never relabelled passes."""
from pathlib import Path
import json,subprocess,os,time,hashlib,sys
ROOT=Path(__file__).resolve().parents[2]
EVIDENCE=ROOT/'output/physics-verification'
OUT=EVIDENCE/time.strftime('%Y%m%d-%H%M%S')
PACKAGED='--packaged' in sys.argv
NATIVE_ONLY='--native-only' in sys.argv
if any(a not in ['--packaged','--native-only'] for a in sys.argv[1:]):raise SystemExit('usage: verify_physics.py [--packaged] [--native-only]')
BUNDLE=ROOT/'output/macOS/MirrorPrismals.app/Contents'
EXE=BUNDLE/'MacOS/wormwood-engine' if PACKAGED else ROOT/'target/debug/wormwood'
ASSETS=BUNDLE/'Resources/EngineAssets' if PACKAGED else ROOT
def run(args,folder):
    folder.mkdir(parents=True,exist_ok=True)
    env=dict(os.environ,WORMWOOD_OUTPUT_ROOT=str(folder),WORMWOOD_ASSET_ROOT=str(ASSETS))
    start=time.monotonic()
    with (folder/'session.log').open('w') as log:
        r=subprocess.run([str(EXE),*args],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=240)
    return r.returncode,time.monotonic()-start
def load(path):return json.loads(path.read_text())
def main():
    assert load(ROOT/'Project.json')['engine']=='Wormwood'
    summary={'game':'Mirror (Prismals!)','engine':'Wormwood','platform':'macOS','packaged':PACKAGED,'stress':[],'native':[]}
    for c in ([] if NATIVE_ONLY else load(ROOT/'SimulationWorkshop/author_cases.json')['cases']):
        case=c['id'];folder=OUT/'stress'/case
        code,seconds=run(['--simulate',case,'--frames','60'],folder)
        file=folder/f'simulations/{case}.json'
        report=load(file)['physics'] if file.exists() else None
        summary['stress'].append({'case':case,'exit':code,'wall_seconds':seconds,'physics':report})
        print(case,code,report.get('simulated_seconds') if report else 'startup failed',flush=True)
    for api in ['OpenGL','Vulkan','WebGPU']:
        for mode in ['workshop','planet']:
            folder=OUT/'native'/f'{mode}-{api}'
            args=['--workshop','cloth_sphere',api,'--frames','36'] if mode=='workshop' else ['--interactive','planet_surface',api,'--verify-session']
            code,seconds=run(args,folder)
            if code:raise RuntimeError(f'{mode}/{api} failed: {folder}/session.log')
            ppm=(folder/f'triangle-{api}.ppm').read_bytes().split(b'\n',3)[3]
            assert len({ppm[i:i+3] for i in range(0,len(ppm),3)})>=3, 'readback lacks scene geometry'
            assert sum(ppm[i]>ppm[i+1]*1.25 and ppm[i]>25 for i in range(0,len(ppm),3))>len(ppm)/300, 'readback lacks visible material pixels'
            report=load(folder/(f'simulations/cloth_sphere-{api}.json' if mode=='workshop' else f'interactive-{api}.json'))
            assert report['entities_remaining']==0
            trace=[json.loads(line) for line in (folder/'execution.jsonl').read_text().splitlines()]
            assert any(row.get('event')=='window_pumped' and row.get('readback_verified') for row in trace)
            row={'mode':mode,'api':api,'exit':code,'wall_seconds':seconds,'entities_remaining':0,'capture_sha256':hashlib.sha256((folder/f'triangle-{api}.ppm').read_bytes()).hexdigest()}
            if mode=='workshop':assert not report['physics']['halted']
            else:
                save=load(folder/'planet/surface.json');assert len(save['edits'])==1 and len(save['particles'])==8
                mass=sum(b['mass'] for b in save['particles']);assert abs(mass-save['removed_volume']*1600)<0.01
                steps=[r for r in trace if r.get('event')=='planet_material_step'];assert steps and all(not r['halted'] for r in steps)
                row.update(edits=1,fragments=8,removed_volume=save['removed_volume'],retained_mass=mass)
            summary['native'].append(row);print(mode,api,'passed',flush=True)
    summary['stress_all_passed']=all(r['exit']==0 for r in summary['stress']) if summary['stress'] else None
    summary['native_all_passed']=True
    OUT.mkdir(parents=True,exist_ok=True);(OUT/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');(EVIDENCE/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(OUT/'summary.json')
    return 1 if summary['stress_all_passed'] is False else 0
if __name__=='__main__':raise SystemExit(main())
