#!/usr/bin/env python3
"""Finite local validation: focused tests, native captures, matched Cycles reference.

Runs in Codex Process Jobs. This does not package, sign or upload an application.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'output' / 'workstation-optics-validation' / datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
OUT.mkdir(parents=True, exist_ok=False)
SOURCES = [ROOT/p for p in [
    'BaseSystem/LayerFour/WorkstationPanelSystem.rs', 'BaseSystem/LayerFive/AudioWorkstationSystem.rs',
    'BaseSystem/LayerOne/GraphicsConnectionSystem.rs', 'Workstation/PanelLayout.rs',
    'Workstation/PanelMotion.rs', 'Workstation/GemMesh.rs', 'Render/Workstation.rs',
    'Workstation/FloorLight.rs',
    'Host/Host.rs', 'Procedures/functionregistry.json',
    'Render/mod.rs', 'Render/ios.rs', 'Render/webgpu.rs', 'Render/vulkan.rs',
    'Render/opengl.rs', 'Render/ios_gl.rs', 'Systems/WorkstationPanelSystem.json',
    'Render/window.rs', 'Render/ios_window.rs', 'BaseSystem/LayerOne/IOSGraphics.m',
    'Systems/AudioWorkstationSystem.json', 'Art/AudioWorkstation/GemPanels/GemPanels_runtime.json',
    'Art/AudioWorkstation/GemPanels/GemPanels_AgX.bin',
    'BuildSupport/Blender/render_workstation_reference.py',
]] + sorted((ROOT/'Procedures/Shaders').glob('workstation*'))
def fingerprints():
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in SOURCES if p.is_file()}
before = fingerprints()
report = {'output': str(OUT), 'source_sha256':before, 'steps':[], 'uploaded':False, 'phone_verified':False}
def save():
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
def run(name, argv, env=None, timeout=2400):
    print(name, flush=True)
    started=time.monotonic()
    with (OUT/(name+'.log')).open('w') as log:
        try:
            result=subprocess.run([str(a) for a in argv],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=timeout)
            status=result.returncode
        except subprocess.TimeoutExpired:
            status=124
    report['steps'].append({'name':name,'status':status,'seconds':round(time.monotonic()-started,3)})
    save()
    print(f'{name}: exit {status}; log {OUT/(name+".log")}',flush=True)
    if status:
        print((OUT/(name+'.log')).read_text(errors='replace')[-7000:],flush=True)
    return status==0

def main():
    if sys.argv[1:]==['--latency-only']:
        # The material/layout checks already passed; only queue pacing changed.
        if not run('native-build',['cargo','build','--offline','--locked','--release','--bin','wormwood']): return 1
        destination=OUT/'WebGPU';destination.mkdir()
        env=os.environ.copy();env['WORMWOOD_ASSET_ROOT']=str(ROOT);env['WORMWOOD_OUTPUT_ROOT']=str(destination);env['WORMWOOD_FULLSCREEN']='0'
        if not run('render-WebGPU',[ROOT/'target/release/wormwood','--workstation','WebGPU','--frames','120'],env=env,timeout=600): return 1
        events=[json.loads(line) for line in (destination/'execution.jsonl').read_text().splitlines()]
        pumps=[e['elapsed_us']/1000 for e in events if e.get('event')=='finished' and e.get('function')=='GraphicsConnectionSystem.Pump']
        median=sorted(pumps)[len(pumps)//2]
        report['latency']={'pumps':len(pumps),'median_pump_ms':median,'last_pump_ms':pumps[-1],
            'total_pump_ms':sum(pumps),'final_queue_drain_limit_ms':max(200.,median*4.)}
        report['source_unchanged']=fingerprints()==before
        report['checks_completed']=len(pumps)==120 and pumps[-1]<=max(200.,median*4.) and report['source_unchanged']
        report['scope']='WebGPU frame pacing only; material, iOS types and other backends were checked in the preceding full validation.'
        save();print(json.dumps(report['latency']),flush=True)
        print(json.dumps({'report':str(OUT/'report.json'),'checks_completed':report['checks_completed'],'uploaded':False}),flush=True)
        return 0 if report['checks_completed'] else 1
    if sys.argv[1:]: raise ValueError('Expected no arguments or --latency-only')
    if not run('focused-tests',['cargo','test','--offline','--locked','--release','--lib','workstation_','--','--nocapture']): return 1
    if not run('native-build',['cargo','build','--offline','--locked','--release','--bin','wormwood']): return 1
    ios_environment=os.environ.copy()
    ios_environment['WORMWOOD_WEBGPU_INCLUDE']=str(ROOT/'output/ios-sdk/wgpu/include/webgpu')
    if not run('ios-type-check',['cargo','check','--offline','--locked','--release','--lib','--target','aarch64-apple-ios'],env=ios_environment): return 1
    if fingerprints()!=before: raise RuntimeError('Workstation sources changed during validation; captures would be ambiguous')
    binary=ROOT/'target/release/wormwood'
    if not run('intent',[binary,'--inspect-intent','--json']): return 1
    intent=json.loads((OUT/'intent.log').read_text())
    report['intent_summary']=intent['summary'];save()
    ok=True
    reference_frame=None
    for backend in ['WebGPU','Vulkan','OpenGL']:
        destination=OUT/backend;destination.mkdir()
        env=os.environ.copy();env['WORMWOOD_ASSET_ROOT']=str(ROOT);env['WORMWOOD_OUTPUT_ROOT']=str(destination);env['WORMWOOD_FULLSCREEN']='0'
        passed=run('render-'+backend,[binary,'--workstation',backend,'--frames','120'],env=env,timeout=600)
        ok=passed and ok
        if passed:
            events=[json.loads(line) for line in (destination/'execution.jsonl').read_text().splitlines()]
            pumps=[e['elapsed_us']/1000 for e in events if e.get('event')=='finished' and e.get('function')=='GraphicsConnectionSystem.Pump']
            report.setdefault('render_timing',{})[backend]={
                'pump_count':len(pumps), 'total_pump_ms':sum(pumps),
                'median_pump_ms':sorted(pumps)[len(pumps)//2] if pumps else None,
                'last_pump_ms':pumps[-1] if pumps else None,
                'note':'CPU submission plus waits/readbacks; asynchronous WebGPU may defer GPU cost to final capture. Not isolated GPU timing.'}
            save()
            frame=destination/'workstation-optical-frame.json'
            packet=json.loads(frame.read_text())
            report.setdefault('render_dimensions',{})[backend]={
                'logical':packet['logical_viewport'], 'framebuffer':packet['rows'][8][:2]}
            save()
            if frame.is_file() and reference_frame is None: reference_frame=frame
            for image in destination.glob('*.ppm'):
                subprocess.run(['/usr/bin/sips','-s','format','png',str(image),'--out',str(image.with_suffix('.png'))],check=True,stdout=subprocess.DEVNULL)
    if reference_frame:
        blender=Path('/Applications/Blender.app/Contents/MacOS/Blender')
        ok=run('blender-reference',[blender,'--background','--factory-startup','--python',ROOT/'BuildSupport/Blender/render_workstation_reference.py','--',reference_frame,OUT/'Blender-reference.png','--samples','128','--max-size','1280','--no-denoise','--bounces','16','--strict-transport'],timeout=1200) and ok
    else:
        ok=False
    report['source_unchanged']=fingerprints()==before
    report['checks_completed']=ok and report['source_unchanged']
    report['visual_review']='pending: examine native and Blender captures; successful rendering alone is not parity'
    save()
    print(json.dumps({'report':str(OUT/'report.json'),'checks_completed':report['checks_completed'],'visual_review':report['visual_review'],'uploaded':False}),flush=True)
    return 0 if report['checks_completed'] else 1

if __name__=='__main__':
    sys.exit(main())
