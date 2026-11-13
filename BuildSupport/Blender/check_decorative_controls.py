"""Focused control-model checks and native captures. No physics suite or upload."""
from pathlib import Path
import os, subprocess, json, datetime, sys
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'output/checks'/('decorative-controls-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
report={'output':str(OUT),'steps':[]}
def run(name,args,env=None,timeout=240):
    print(name,flush=True)
    with (OUT/(name+'.log')).open('w') as log:
        result=subprocess.run(args,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,env=env,timeout=timeout)
    report['steps'].append({'name':name,'exit_code':result.returncode})
    (OUT/'report.json').write_text(json.dumps(report,indent=2))
    if result.returncode:
        print((OUT/(name+'.log')).read_text()[-6000:],flush=True)
        raise SystemExit(result.returncode)
if '--captures-only' not in sys.argv:
    run('focused-checks',['cargo','test','--offline','--locked','--release','--lib','workstation_','--','--nocapture'],timeout=360)
run('desktop-build',['cargo','build','--offline','--locked','--release','--bin','wormwood'],timeout=360)
for backend in ['WebGPU','OpenGL','Vulkan']:
    dest=OUT/backend;dest.mkdir();env=os.environ.copy();env.update(WORMWOOD_ASSET_ROOT=str(ROOT),WORMWOOD_OUTPUT_ROOT=str(dest),WORMWOOD_FULLSCREEN='0')
    run('render-'+backend,[str(ROOT/'target/release/wormwood'),'--workstation',backend,'--frames','5'],env,120)
    for p in dest.glob('*.ppm'):
        subprocess.run(['sips','-s','format','png',str(p),'--out',str(p.with_suffix('.png'))],check=True,stdout=subprocess.DEVNULL)
print(json.dumps(report),flush=True)
