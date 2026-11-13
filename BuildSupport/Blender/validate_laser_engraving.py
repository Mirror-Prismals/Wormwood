#!/usr/bin/env python3
"""Finite SSLE asset/build/capture/package pass. No uploads or unrelated tests."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'output/checks' / ('laser-engraving-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
report = {'output': str(OUT), 'steps': [], 'uploaded': False, 'visual_review': 'pending'}

def save():
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

def run(name, argv, timeout, env=None):
    print(name, flush=True)
    start = time.monotonic()
    with (OUT / (name + '.log')).open('w') as log:
        try:
            result = subprocess.run([str(x) for x in argv], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
            status = result.returncode
        except subprocess.TimeoutExpired:
            status = 124
    report['steps'].append({'name': name, 'status': status, 'seconds': round(time.monotonic()-start, 2)})
    save()
    if status:
        print((OUT / (name + '.log')).read_text(errors='replace')[-7000:], flush=True)
        raise SystemExit(status)

if sys.argv[1:] in (['--reuse-assets'], ['--render-only']):
    assets = ROOT / 'Art/AudioWorkstation/LaserEngraving'
    manifest = json.loads((assets / 'engraving.json').read_text())
    assert hashlib.sha256((assets / 'engraving.bin').read_bytes()).hexdigest() == manifest['binary_sha256']
    assert hashlib.sha256((assets / 'density.bin').read_bytes()).hexdigest() == manifest['density_sha256']
    checks = json.loads((assets / 'engraving_checks.json').read_text())
    assert len(checks['embedding_checks']) == 6 and all(x['all_fracture_vertices_inside'] for x in checks['embedding_checks'])
    assert (assets / 'GemPanels_engraved.blend').is_file()
    report['reused_verified_blender_assets'] = True
elif sys.argv[1:]:
    raise SystemExit('Expected no arguments, --reuse-assets or --render-only')
else:
    run('blender-author', ['/Applications/Blender.app/Contents/MacOS/Blender', '--background', '--factory-startup', '--python-exit-code', '1', '--python', 'BuildSupport/Blender/make_laser_engraving.py'], 240)
if sys.argv[1:] != ['--render-only']:
    run('ssle-containment', ['cargo', 'test', '--offline', '--locked', '--release', '--lib', 'workstation_ssle_', '--', '--nocapture'], 300)
else:
    report['containment'] = 'Previously passed; only shader variable naming changed.'
run('native-build', ['cargo', 'build', '--offline', '--locked', '--release', '--bin', 'wormwood'], 300)
for backend in ['WebGPU', 'Vulkan', 'OpenGL']:
    for retina in ['0', '1']:
        name = backend + ('-Retina' if retina == '1' else '')
        dest = OUT / name
        dest.mkdir()
        env = os.environ.copy()
        env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(dest),
                   WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA=retina)
        frames = '36' if backend == 'WebGPU' else '6'
        run('render-' + name, [ROOT / 'target/release/wormwood', '--workstation', backend, '--frames', frames], 90, env)
        for path in dest.glob('*.ppm'):
            subprocess.run(['sips', '-s', 'format', 'png', str(path), '--out', str(path.with_suffix('.png'))], check=True, stdout=subprocess.DEVNULL)
        if backend == 'WebGPU':
            events = [json.loads(line) for line in (dest / 'execution.jsonl').read_text().splitlines()]
            times = [e['elapsed_us']/1000 for e in events if e.get('event') == 'finished' and e.get('function') == 'GraphicsConnectionSystem.Pump'][6:]
            report.setdefault('timing', {})[name] = {'median_pump_ms': statistics.median(times), 'measured_frames': len(times)}
            save()
before = ROOT / 'output/checks/ssle-style-before'
if (before / 'engraving.json').exists():
    previous = json.loads((before / 'engraving.json').read_text())
    current = json.loads((ROOT / 'Art/AudioWorkstation/LaserEngraving/engraving.json').read_text())
    def layout(m):
        return [(b['panel_index'], b['lines']) for b in m['demo_blocks']]
    report['style_scope'] = {
        'original_advances_unchanged': [g['advance_em'] for g in previous['glyphs']] == [g['advance_em'] for g in current['glyphs']],
        'font_unchanged': previous['font_sha256'] == current['font_sha256'],
        'text_sizes_and_baselines_unchanged': layout(previous) == layout(current),
    }
    assert all(report['style_scope'].values()), report['style_scope']
    save()
assets = ROOT / 'Art/AudioWorkstation/LaserEngraving'
manifest = json.loads((assets / 'engraving.json').read_text())
assert hashlib.sha256((assets / 'density.bin').read_bytes()).hexdigest() == manifest['density_sha256']
run('package-mac', ['python3', 'BuildSupport/macOS/build_app.py'], 180)
report['blender_checks'] = json.loads((ROOT / 'Art/AudioWorkstation/LaserEngraving/engraving_checks.json').read_text())
report['ready_for_visual_review'] = True
save()
print(json.dumps({'report': str(OUT / 'report.json'), 'steps': report['steps'], 'visual_review': report['visual_review']}), flush=True)
