"""Stage the Mac app and capture its engraved transport and visible mixer controls.

The normal local app is replaced only after these captures receive visual review.
No upload, interactive app launch, or full simulation suite is performed here.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked

OUT = ROOT / 'output/checks' / ('control-engraving-package-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
report = {'output': str(OUT), 'steps': [], 'installed': False, 'uploaded': False,
          'visual_review': 'pending'}


def save():
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


def run(name, argv, timeout=120, env=None):
    print(name, flush=True)
    started = time.monotonic()
    try:
        run_checked([str(x) for x in argv], cwd=ROOT, log=OUT / (name + '.log'), timeout=timeout, env=env)
        status = 0
    except subprocess.CalledProcessError as error:
        status = error.returncode
    except subprocess.TimeoutExpired:
        status = 124
    report['steps'].append({'name': name, 'status': status, 'seconds': round(time.monotonic()-started, 3)})
    save()
    if status:
        print((OUT / (name + '.log')).read_text(errors='replace')[-7000:], flush=True)
        raise SystemExit(status)


run('generate-shaders', ['python3', 'Procedures/Shaders/generate_workstation.py'], 60)
run('package-mac', ['python3', 'BuildSupport/macOS/build_app.py', '--output-dir', OUT / 'package'], 600)
project = json.loads((ROOT / 'Project.json').read_text())
app = OUT / 'package' / (project['name'] + '.app')
assets = app / 'Contents/Resources/EngineAssets'
exe = app / 'Contents/MacOS/wormwood-engine'
engraving = assets / 'Art/AudioWorkstation/ControlEngraving'
manifest = json.loads((engraving / 'control_engraving.json').read_text())
assert hashlib.sha256((engraving / manifest['density_binary']).read_bytes()).hexdigest() == manifest['density_sha256']
report['app'] = str(app)
report['executable_sha256'] = hashlib.sha256(exe.read_bytes()).hexdigest()
report['runtime_sha256'] = {str(p.relative_to(assets)): hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in sorted(assets.rglob('*')) if p.is_file()}
save()
env = os.environ.copy()
for key in ['WORMWOOD_STYLE', 'WORMWOOD_WORKSTATION_GPU_TIMING', 'WORMWOOD_WORKSTATION_CAPTURE_DT']:
    env.pop(key, None)
env.update(WORMWOOD_ASSET_ROOT=str(assets), WORMWOOD_OUTPUT_ROOT=str(OUT / 'startup'),
           WORMWOOD_PERFORMANCE='0', WORMWOOD_FULLSCREEN='0')
run('runtime-startup', [exe], 30, env)
for backend, retina in [('WebGPU', '0'), ('WebGPU', '1'), ('Vulkan', '0'), ('OpenGL', '0')]:
    name = backend + ('-Retina' if retina == '1' else '')
    dest = OUT / name
    dest.mkdir()
    env.update(WORMWOOD_OUTPUT_ROOT=str(dest), WORMWOOD_WORKSTATION_RETINA=retina)
    run('capture-' + name, [exe, '--check-workstation-controls', backend], 150, env)
    diagnostic = json.loads((dest / ('workstation-controls-' + backend + '.json')).read_text())
    assert diagnostic['complete'] and diagnostic['mixer_controls_visible'], 'Mixer diagnostic incomplete'
    frame = json.loads((dest / 'workstation-optical-frame.json').read_text())
    rows = frame['rows']
    assert rows[244][1] == manifest['density_rows'], 'Engraving density header missing'
    assert all(rows[192 + index * 2][3] > 0 for index in range(10)), 'Transport or mixer controls are hidden'
    capture = dest / ('triangle-' + backend + '.ppm')
    assert capture.is_file() and capture.stat().st_size > 32, 'Native capture missing'
    run('png-' + name, ['sips', '-s', 'format', 'png', capture, '--out', capture.with_suffix('.png')], 20)
    report.setdefault('captures', {})[name] = {'dimensions': rows[8][:2], 'diagnostic': diagnostic,
        'visible_control_count': 10, 'png': str(capture.with_suffix('.png'))}
    save()
run('verify-app-signature', ['codesign', '--verify', '--deep', '--strict', app], 30)
assert hashlib.sha256(exe.read_bytes()).hexdigest() == report['executable_sha256'], 'Executable changed during checks'
for relative, digest in report['runtime_sha256'].items():
    assert hashlib.sha256((assets / relative).read_bytes()).hexdigest() == digest, ('Packaged input changed', relative)
report['ready_for_visual_review'] = True
save()
print(json.dumps({'report': str(OUT / 'report.json'), 'app': str(app),
                  'installed': False, 'visual_review': 'pending'}), flush=True)
