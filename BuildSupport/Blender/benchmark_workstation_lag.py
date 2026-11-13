"""Finite, matched before/after optical captures and Mac packaging.

The saved baseline executable is required for Vulkan's embedded SPIR-V.
Finite --frames explicitly keeps rendering in the new idle-aware event loop;
its Pump timings must not be mistaken for skipped idle ticks.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked

BASELINE = ROOT / 'output/checks/workstation-lag-baseline-20261030'
OUT = ROOT / 'output/checks' / ('workstation-lag-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
report = {'output': str(OUT), 'steps': [], 'uploaded': False,
          'timing_scope': 'Short CPU Pump timings including GPU waits; finite diagnostics force actual draws. Not isolated GPU timestamps or sustained FPS.',
          'visual_review': 'pending'}

def save():
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

def run(name, args, timeout=240, env=None):
    print(name, flush=True)
    started = time.monotonic()
    try:
        run_checked([str(a) for a in args], cwd=ROOT, log=OUT / (name + '.log'), timeout=timeout, env=env)
        status = 0
    except subprocess.CalledProcessError as error:
        status = error.returncode
    except subprocess.TimeoutExpired:
        status = 124
    report['steps'].append({'name': name, 'status': status, 'seconds': round(time.monotonic() - started, 3)})
    save()
    if status:
        print((OUT / (name + '.log')).read_text(errors='replace')[-6000:], flush=True)
        raise SystemExit(status)

def source_hashes():
    paths = list((ROOT / 'Procedures/Shaders').glob('workstation*'))
    paths += [ROOT / p for p in ['Render/mod.rs', 'Render/window.rs',
              'BaseSystem/LayerFive/AudioWorkstationSystem.rs',
              'BaseSystem/LayerOne/GraphicsConnectionSystem.rs',
              'Art/AudioWorkstation/DecorativeControls/controls.bin',
              'Art/AudioWorkstation/DecorativeControls/controls.json']]
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}

def baseline_assets():
    assets = OUT / 'baseline-assets'
    assets.mkdir()
    # Copy only the two edited asset directories, link other resources.
    overrides = {
        'Procedures/Shaders': [p.name for p in (ROOT / 'Procedures/Shaders').glob('workstation*')],
        'Art/AudioWorkstation/DecorativeControls': ['controls.bin', 'controls.json'],
    }
    parents = {'Procedures', 'Art', 'Art/AudioWorkstation'}
    def populate(relative=''):
        for p in (ROOT / relative).iterdir():
            rel = str(p.relative_to(ROOT))
            target = assets / rel
            if rel in parents or rel in overrides:
                target.mkdir()
                populate(rel)
            elif relative in overrides and p.name in overrides[relative]:
                saved = BASELINE / p.name
                if not saved.is_file():
                    raise RuntimeError(f'Missing saved baseline asset: {saved}')
                shutil.copy2(saved, target)
            else:
                target.symlink_to(p, target_is_directory=p.is_dir())
    populate()
    return assets

run('control-intersections', ['python3', 'BuildSupport/Blender/check_control_acceleration.py'], 120)
before = source_hashes()
run('workstation-tests', ['cargo', 'test', '--offline', '--locked', '--release', '--lib', 'workstation_', '--', '--nocapture'], 300)
run('native-build', ['cargo', 'build', '--offline', '--locked', '--release', '--bin', 'wormwood'], 300)
run('intent', [ROOT / 'target/release/wormwood', '--inspect-intent', '--json'], 30)
assert source_hashes() == before, 'Workstation sources changed during build'
old_assets = baseline_assets()
assert (BASELINE / 'wormwood').is_file(), 'Missing saved pre-change executable'

for backend in ['WebGPU', 'Vulkan', 'OpenGL']:
    for variant, binary, assets in [('before', BASELINE / 'wormwood', old_assets),
                                     ('after', ROOT / 'target/release/wormwood', ROOT)]:
        name = backend + '-' + variant
        dest = OUT / name
        dest.mkdir()
        env = os.environ.copy()
        env.update(WORMWOOD_ASSET_ROOT=str(assets), WORMWOOD_OUTPUT_ROOT=str(dest),
                   WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA='0')
        run(name, [binary, '--workstation', backend, '--frames', '48'], 120, env)
        events = [json.loads(line) for line in (dest / 'execution.jsonl').read_text().splitlines()]
        times = [e['elapsed_us'] / 1000 for e in events if e.get('event') == 'finished' and e.get('function') == 'GraphicsConnectionSystem.Pump']
        assert len(times) == 48, (name, len(times))
        report.setdefault('timing_ms', {})[name] = {'frames': times, 'warm_median': statistics.median(times[16:-1])}
        packet = json.loads((dest / 'workstation-optical-frame.json').read_text())
        report.setdefault('dimensions', {})[name] = {'logical': packet['logical_viewport'], 'framebuffer': packet['rows'][8][:2]}
        for path in dest.glob('*.ppm'):
            subprocess.run(['sips', '-s', 'format', 'png', str(path), '--out', str(path.with_suffix('.png'))], check=True, stdout=subprocess.DEVNULL)
        save()
    old_frame = json.loads((OUT / (backend + '-before') / 'workstation-optical-frame.json').read_text())
    new_frame = json.loads((OUT / (backend + '-after') / 'workstation-optical-frame.json').read_text())
    assert old_frame['rows'][:12544] == new_frame['rows'][:12544], f'{backend} layout, lights, materials or sampling changed'
    assert old_frame['rows'][45312:] == new_frame['rows'][45312:], f'{backend} engraving changed'
    old = (OUT / (backend + '-before') / ('triangle-' + backend + '.ppm')).read_bytes()
    new = (OUT / (backend + '-after') / ('triangle-' + backend + '.ppm')).read_bytes()
    old_header, old_pixels = old.split(b'\n255\n', 1)
    new_header, new_pixels = new.split(b'\n255\n', 1)
    assert old_header == new_header and len(old_pixels) == len(new_pixels)
    differences = [abs(a-b) for a, b in zip(old_pixels, new_pixels)]
    comparison = {'identical': old == new, 'mean_absolute_byte_difference': statistics.mean(differences),
                  'changed_channels': sum(d > 0 for d in differences), 'max_difference': max(differences)}
    report.setdefault('capture_comparison', {})[backend] = comparison
    report.setdefault('speedup', {})[backend] = report['timing_ms'][backend + '-before']['warm_median'] / report['timing_ms'][backend + '-after']['warm_median']
    save()
    assert comparison['mean_absolute_byte_difference'] < 0.1, f'{backend} appearance changed: {comparison}'

# Exercise the ordinary unbounded application loop, closing through its existing
# launcher stop-file protocol. The deadline is owned by this finite harness;
# no input injection or changes to the user's running session are needed.
for backend in ['WebGPU', 'Vulkan', 'OpenGL']:
    name = backend + '-idle'
    dest = OUT / name
    dest.mkdir()
    stop = dest / 'session-stop-idle-check'
    env = os.environ.copy()
    env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(dest),
               WORMWOOD_FULLSCREEN='1' if backend == 'WebGPU' else '0',
               WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_STOP_FILE=str(stop))
    timer = threading.Timer(25, stop.touch)
    timer.start()
    try:
        run(name, [ROOT / 'target/release/wormwood', '--workstation', backend], 60, env)
    finally:
        timer.cancel()
    events = [json.loads(line) for line in (dest / 'execution.jsonl').read_text().splitlines()]
    pumped = [e for e in events if e.get('event') == 'window_pumped']
    skipped = sum(not e['drawn'] and not e['closed'] for e in pumped)
    times = [e['elapsed_us']/1000 for e in events if e.get('event') == 'finished' and e.get('function') == 'GraphicsConnectionSystem.Pump']
    final = json.loads((dest / ('workstation-' + backend + '.json')).read_text())
    report.setdefault('idle', {})[backend] = {
        'fullscreen': backend == 'WebGPU', 'input_ticks': len(pumped),
        'skipped_gpu_draws': skipped, 'last_60_pump_median_ms': statistics.median(times[-60:]),
        'end_reason': final['end_reason'], 'entities_remaining': final['entities_remaining']}
    save()
    assert skipped >= 60, f'{backend} did not reach idle input polling: {report["idle"][backend]}'
    assert final['end_reason'] == 'window_closed' and final['entities_remaining'] == 0

assert source_hashes() == before, 'Workstation sources changed during comparison'
run('package-mac', ['python3', 'BuildSupport/macOS/build_app.py'], 180)
run('verify-signature', ['codesign', '--verify', '--deep', '--strict', ROOT / 'output/macOS/MirrorPrismals.app'], 30)
report['source_sha256'] = before
report['ready_for_visual_review'] = True
save()
print(json.dumps({'report': str(OUT / 'report.json'), 'timing_ms': {k: v['warm_median'] for k, v in report['timing_ms'].items()},
                  'speedup': report['speedup'], 'visual_review': report['visual_review']}), flush=True)
