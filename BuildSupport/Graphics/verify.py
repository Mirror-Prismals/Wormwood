#!/usr/bin/env python3
"""Finite native build and per-backend pixel/lifecycle verification."""
from pathlib import Path
import json, os, shutil, subprocess, sys, time

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'output'
OUT.mkdir(exist_ok=True)
SUMMARY = OUT / 'graphics-verification.json'
report = {'started_at_unix': time.time(), 'build': 'pending', 'backends': {}}
def save():
    SUMMARY.write_text(json.dumps(report, indent=2) + '\n')
def command(argv, timeout):
    print('+ ' + ' '.join(str(a) for a in argv), flush=True)
    result = subprocess.run(argv, cwd=ROOT, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'command failed ({result.returncode}): {argv[0]}')
def verify_pixels(path):
    magic, size, maximum, pixels = path.read_bytes().split(b'\n', 3)
    assert magic == b'P6' and maximum == b'255', 'unexpected readback encoding'
    w, h = map(int, size.split())
    assert len(pixels) == w * h * 3 and w > 0 and h > 0
    center = (h // 2 * w + w // 2) * 3
    rgb = pixels[center:center+3]
    assert all(c > 35 for c in rgb), f'center is not triangle color: {list(rgb)}'
    assert all(c < 30 for c in pixels[:3]), 'corner is not the declared background'
    return {'width': w, 'height': h, 'center_rgb': list(rgb), 'corner_rgb': list(pixels[:3])}
def verify_trace():
    rows = [json.loads(line) for line in (OUT / 'execution.jsonl').read_text().splitlines()]
    run_id = rows[-1]['run_id']
    rows = [e for e in rows if e['run_id'] == run_id]
    assert not any(e['event'] in ('failed', 'rejected', 'audicle_cancelled') for e in rows)
    stages = [e['stage'] for e in rows if e['event'] == 'audicle_stage']
    assert stages == ['Preparing', 'Ready', 'Go', 'Finished'], stages
    def index(kind, stage=None):
        return next(i for i,e in enumerate(rows) if e['event'] == kind and (stage is None or e.get('stage') == stage))
    assert index('entity_created') < index('native_prepared') < index('audicle_stage','Ready')
    assert index('audicle_stage','Go') < index('frame_presented')
    assert index('native_released') < index('entity_removed') < index('audicle_stage','Finished')
    assert all(e['audicle'] is not None for e in rows if e['event'] == 'frame_presented')
    assert all(e['visible'] is False for e in rows if e['event'] == 'native_prepared')
    starts = {e['invocation']: e for e in rows if e['event'] == 'started'}
    for e in rows:
        if e['event'] == 'started' and e['parent'] is not None:
            assert e['parent'] in starts, 'missing parent invocation'
    return {'run_id':run_id,'events':len(rows),'stages':stages}

save()
try:
    command(['cargo','fmt','--check'], 30)
    command(['cargo','test','--offline'], 300)
    command(['cargo','build','--offline'], 300)
    report['build'] = 'passed'
except Exception as error:
    report['build'] = 'failed'
    report['error'] = str(error)
    save()
    raise
save()
for backend in ['OpenGL','Vulkan','WebGPU']:
    image = OUT / f'triangle-{backend}.ppm'
    result_file = OUT / f'triangle-{backend}.json'
    image.unlink(missing_ok=True)
    result_file.unlink(missing_ok=True)
    try:
        argv = [str(ROOT/'target/debug/wormwood'),'--triangle',backend,'--frames','6']
        print(f'Verifying native {backend}', flush=True)
        result = subprocess.run(argv,cwd=ROOT,capture_output=True,text=True,timeout=45)
        (OUT/f'triangle-{backend}.stdout.log').write_text(result.stdout)
        (OUT/f'triangle-{backend}.stderr.log').write_text(result.stderr)
        assert result.returncode == 0, f'exit {result.returncode}: {result.stderr[-3000:]}'
        cue = json.loads(result_file.read_text())
        assert cue['stage'] == 'Finished' and cue['entities_remaining'] == 0
        assert cue['presentation']['backend'] == backend and cue['presentation']['frames'] == 6
        assert cue['presentation']['device']['readback_verified'] is True
        pixels = verify_pixels(image)
        trace = verify_trace()
        png = OUT/f'triangle-{backend}.png'
        if shutil.which('sips'):
            subprocess.run(['sips','-s','format','png',str(image),'--out',str(png)],check=True,capture_output=True,timeout=20)
        report['backends'][backend] = {'status':'passed','presentation':cue['presentation'],'pixels':pixels,'trace':trace}
        print(f'{backend}: presented 6 frames, pixel readback and lifecycle verified', flush=True)
    except Exception as error:
        report['backends'][backend] = {'status':'failed','error':str(error)}
        print(f'{backend}: FAILED: {error}', flush=True)
    save()
report['finished_at_unix'] = time.time()
report['passed'] = all(v['status'] == 'passed' for v in report['backends'].values())
save()
print(json.dumps(report,indent=2), flush=True)
sys.exit(0 if report['passed'] else 1)
