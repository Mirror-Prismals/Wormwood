"""Bounded Mac before/after captures at identical full optical quality."""
from pathlib import Path
import datetime, json, os, shutil, statistics, subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'output/checks'/('control-acceleration-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'));OUT.mkdir(parents=True)
BASELINE=ROOT/'output/checks/control-performance-baseline'
report={'output':str(OUT),'quality':'unchanged samples, depth, events, materials and source GLBs','steps':[],
        'comparison_limits':{'Vulkan':'Current render only: SPIR-V is embedded in the executable, so changing the asset root cannot select a baseline shader.',
                             'timing':'Short CPU pump timings, not GPU timestamps or sustained frame rates; OpenGL may queue work asynchronously.'}}
def save(): (OUT/'report.json').write_text(json.dumps(report,indent=2))
def run(name,args,env=None,timeout=300):
 print(name,flush=True)
 try:
  with (OUT/(name+'.log')).open('w') as log:r=subprocess.run([str(a) for a in args],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=timeout)
 except subprocess.TimeoutExpired:
  report['steps'].append({'name':name,'timeout':timeout});save();raise
 report['steps'].append({'name':name,'exit_code':r.returncode});save()
 if r.returncode:
  print((OUT/(name+'.log')).read_text()[-5000:],flush=True);raise SystemExit(r.returncode)
run('build',['cargo','build','--offline','--locked','--release','--bin','wormwood'])
# The baseline uses the original shader and original BVH packet. Mesh geometry,
# poses and optical quality are identical; acceleration metadata differs.
assets=OUT/'baseline-assets';assets.mkdir()
for p in ROOT.iterdir():
 if p.name not in ['Procedures','Art']:(assets/p.name).symlink_to(p,target_is_directory=p.is_dir())
(assets/'Art/AudioWorkstation/DecorativeControls').mkdir(parents=True)
for p in (ROOT/'Art').iterdir():
 if p.name!='AudioWorkstation':(assets/'Art'/p.name).symlink_to(p,target_is_directory=p.is_dir())
for p in (ROOT/'Art/AudioWorkstation').iterdir():
 if p.name!='DecorativeControls':(assets/'Art/AudioWorkstation'/p.name).symlink_to(p,target_is_directory=p.is_dir())
shutil.copy2(BASELINE/'controls.bin',assets/'Art/AudioWorkstation/DecorativeControls/controls.bin')
manifest=json.loads((BASELINE/'controls.json').read_text());manifest['schema']=2
# Schema wrapper enables the old immutable packet in the current loader; the old
# shader never reads the new per-material acceleration fields.
(assets/'Art/AudioWorkstation/DecorativeControls/controls.json').write_text(json.dumps(manifest))
(assets/'Procedures').mkdir()
for p in (ROOT/'Procedures').iterdir():
 if p.name!='Shaders':(assets/'Procedures'/p.name).symlink_to(p,target_is_directory=p.is_dir())
shaders=assets/'Procedures/Shaders';shaders.mkdir()
for p in (ROOT/'Procedures/Shaders').iterdir():
 old=BASELINE/p.name
 if old.is_file():shutil.copy2(old,shaders/p.name)
 else:(shaders/p.name).symlink_to(p,target_is_directory=p.is_dir())
for backend in ['WebGPU','Vulkan','OpenGL']:
 # Vulkan embeds its shader at compile time. Pairing that current shader with
 # the old packet is invalid because its acceleration metadata has changed.
 variants=[('after',ROOT)] if backend=='Vulkan' else [('before',assets),('after',ROOT)]
 for variant,root in variants:
  dest=OUT/(backend+'-'+variant);dest.mkdir();env=os.environ.copy();env.update(WORMWOOD_ASSET_ROOT=str(root),WORMWOOD_OUTPUT_ROOT=str(dest),WORMWOOD_FULLSCREEN='0')
  run(backend+'-'+variant,[ROOT/'target/release/wormwood','--workstation',backend,'--frames','12'],env,90)
  events=[json.loads(line) for line in (dest/'execution.jsonl').read_text().splitlines()]
  times=[e['elapsed_us']/1000 for e in events if e.get('event')=='finished' and e.get('function')=='GraphicsConnectionSystem.Pump']
  report.setdefault('timing_ms',{})[backend+'-'+variant]={'frames':times,'warm_median':statistics.median(times[3:-1])};save()
  for p in dest.glob('*.ppm'):subprocess.run(['sips','-s','format','png',str(p),'--out',str(p.with_suffix('.png'))],check=True,stdout=subprocess.DEVNULL)
 if backend!='Vulkan':
  before=(OUT/(backend+'-before')/('triangle-'+backend+'.ppm')).read_bytes()
  after=(OUT/(backend+'-after')/('triangle-'+backend+'.ppm')).read_bytes()
  report.setdefault('identical_captures',{})[backend]=before==after
  if before==after:
   report.setdefault('speedup',{})[backend]=report['timing_ms'][backend+'-before']['warm_median']/report['timing_ms'][backend+'-after']['warm_median']
  save()
run('package-mac',['python3','BuildSupport/macOS/build_app.py'],timeout=180)
print(json.dumps(report),flush=True)
