"""Bounded, full-resolution profiling and faithful-renderer experiments.

Private diagnostic variants isolate costs; none are installed in the app.
The production candidate retains all meshes/materials/samples/path budgets.
"""
from pathlib import Path
import array
import argparse
import datetime
import hashlib
import json
import os
import shutil
import statistics
import struct
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked

BASE = ROOT / 'output/checks/workstation-fidelity-baseline-20261030'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--reuse-build', type=Path, help='Completed validation report directory with a frozen executable; resume measurements without repeating passed checks.')
parser.add_argument('--asset-root', type=Path, default=ROOT, help='Private snapshot of runtime inputs; defaults to the checkout for a new build.')
ARGS = parser.parse_args()
SOURCE_ASSETS = ARGS.asset_root.resolve()
OUT = ROOT / 'output/checks' / ('workstation-profile-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
REPORT = {'output': str(OUT), 'steps': [], 'packaged': False, 'uploaded': False,
          'quality': 'Production candidate keeps native resolution, authored geometry/materials, engraved text and all optical budgets.',
          'diagnostic_variants': 'Private cost-isolation shaders only; not installed or shipped.'}

def runtime_files():
    files = set()
    for pattern in ['Project.json', 'Systems/*.json', 'BaseSystem/registry.json', 'BaseSystem/circuit_breaker.json',
                    'Procedures/functionregistry.json', 'Procedures/systems.json',
                    'Procedures/player_controls.json', 'Procedures/caves.json',
                    'Procedures/Shaders/workstation.wgsl',
                    'Procedures/Shaders/workstation.gl.vert', 'Procedures/Shaders/workstation.gl.frag',
                    'Art/AudioWorkstation/GemPanels/*.glb',
                    'Art/AudioWorkstation/GemPanels/GemPanels_runtime.json',
                    'Art/AudioWorkstation/GemPanels/GemPanels_AgX.bin',
                    'Art/AudioWorkstation/DecorativeControls/controls.*',
                    'Art/AudioWorkstation/LaserEngraving/engraving.json',
                    'Art/AudioWorkstation/LaserEngraving/*.bin',
                    'Art/AudioWorkstation/InputMonitor/runtime.*',
                    'Art/AudioWorkstation/EdisonHardware/runtime.*',
                    'Art/AudioWorkstation/GemButtons/*.glb',
                    'Art/AudioWorkstation/GemFadersMeters/*.glb']:
        files.update(SOURCE_ASSETS.glob(pattern))
    return sorted(str(p.relative_to(SOURCE_ASSETS)) for p in files if p.is_file())

def save():
    (OUT / 'report.json').write_text(json.dumps(REPORT, indent=2) + '\n')

def fingerprint():
    files = set()
    for pattern in ['Render/*.rs', 'Workstation/*.rs',
                    'Project.json', 'Systems/*.json', 'BaseSystem/registry.json',
                    'BaseSystem/circuit_breaker.json', 'Procedures/functionregistry.json',
                    'Procedures/systems.json',
                    'BaseSystem/LayerFive/AudioWorkstationSystem.rs',
                    'BaseSystem/LayerFour/WorkstationPanelSystem.rs',
                    'BaseSystem/LayerTwo/WorkstationDataSystem.rs',
                    'Procedures/Shaders/workstation*',
                    'Procedures/Shaders/generate_workstation.py',
                    'Art/AudioWorkstation/**/*.bin', 'Art/AudioWorkstation/**/*.json', 'Art/AudioWorkstation/**/*.glb',
                    'BuildSupport/Workstation/*', 'BuildSupport/Blender/*.py']:
        files.update(p for p in ROOT.glob(pattern) if p.is_file())
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(files)}

def verify_sources():
    actual = fingerprint()
    expected = REPORT['source_hashes']
    changed = sorted(p for p in actual.keys() | expected.keys() if actual.get(p) != expected.get(p))
    if changed:
        REPORT['concurrent_source_edits'] = changed
        save()
    # Both variants use copied runtime inputs and the same frozen executable.
    # Ongoing authoring elsewhere in the shared checkout cannot alter the run.
    for relative, digest in REPORT.get('frozen_runtime_sha256', {}).items():
        assert hashlib.sha256((OUT / relative).read_bytes()).hexdigest() == digest, ('Frozen runtime input changed', relative)
    if 'executable_sha256' in REPORT:
        assert hashlib.sha256((OUT / 'wormwood-profile').read_bytes()).hexdigest() == REPORT['executable_sha256']

def run(name, argv, timeout=180, env=None):
    print(name, flush=True)
    start = time.monotonic()
    try:
        run_checked([str(x) for x in argv], cwd=ROOT, log=OUT / (name + '.log'), timeout=timeout, env=env)
        status = 0
    except subprocess.CalledProcessError as error:
        status = error.returncode
    except subprocess.TimeoutExpired:
        status = 124
    REPORT['steps'].append({'name': name, 'status': status, 'seconds': round(time.monotonic() - start, 3)})
    save()
    if status:
        print((OUT / (name + '.log')).read_text(errors='replace')[-7000:], flush=True)
        raise SystemExit(status)

def frozen_assets(name, baseline=False):
    destination = OUT / name
    destination.mkdir()
    files = set(runtime_files())
    parents = {str(parent) for path in files for parent in Path(path).parents if str(parent) != '.'}
    def populate(relative=''):
        for path in (SOURCE_ASSETS / relative).iterdir():
            rel = str(path.relative_to(SOURCE_ASSETS))
            target = destination / rel
            if rel in parents:
                target.mkdir()
                populate(rel)
            elif rel in files:
                source = BASE / path.name if baseline and rel in {
                    'Art/AudioWorkstation/DecorativeControls/controls.json',
                    'Art/AudioWorkstation/DecorativeControls/controls.bin'} else (CURRENT_ASSETS / rel if baseline else path)
                shutil.copy2(source, target)
                if baseline and rel == 'Procedures/Shaders/workstation.wgsl':
                    shader = target.read_text()
                    assert shader.count('if(tree.z>0.0&&bandSafe)') == 1
                    # Reference the full original BVH while retaining the same
                    # current scene/material code. The compiler can eliminate
                    # the unused candidate helper instead of charging its cost
                    # to both sides of the comparison.
                    target.write_text(shader.replace('if(tree.z>0.0&&bandSafe)', 'if(false)'))
                REPORT.setdefault('frozen_runtime_sha256', {})[str(target.relative_to(OUT))] = hashlib.sha256(target.read_bytes()).hexdigest()
            else:
                target.symlink_to(path, target_is_directory=path.is_dir())
    populate()
    return destination

def frame_binary(frame, filename):
    rows = frame['rows']
    assert len(rows) == 49408 and rows[7][0] == 49408 and rows[224][0] == 234601
    packed = array.array('f', (v for row in rows for v in row))
    if sys.byteorder != 'little':
        packed.byteswap()
    data = packed.tobytes()
    data += (CURRENT_ASSETS / 'Art/AudioWorkstation/GemPanels/GemPanels_AgX.bin').read_bytes()
    data += (CURRENT_ASSETS / 'Art/AudioWorkstation/LaserEngraving/density.bin').read_bytes()
    for name in ['InputMonitor', 'EdisonHardware']:
        directory = CURRENT_ASSETS / 'Art/AudioWorkstation' / name
        if not directory.exists():
            continue
        manifest = json.loads((directory / 'runtime.json').read_text())
        start = int(manifest['base']) * 16
        end = start + int(manifest['capacity']) * 16
        binary = (directory / 'runtime.bin').read_bytes()
        assert len(binary) == int(manifest['rows']) * 16 and len(binary) <= end-start
        assert len(data) <= start, ('Overlapping immutable extension', name)
        data += bytes(end-len(data))
        data = data[:start] + binary + data[start+len(binary):]
    data += bytes((-len(data)) % (256 * 16))
    REPORT['optical_buffer_bytes'] = len(data)
    filename.write_bytes(data)

def compare_images(before, after):
    h1, p1 = before.read_bytes().split(b'\n255\n', 1)
    h2, p2 = after.read_bytes().split(b'\n255\n', 1)
    assert h1 == h2 and len(p1) == len(p2)
    differences = [abs(a-b) for a, b in zip(p1, p2)]
    return {'changed_channels': sum(d != 0 for d in differences),
            'mean_absolute_byte_difference': statistics.mean(differences),
            'maximum_byte_difference': max(differences)}

def change_function(source, name, transform):
    start = source.index('fn ' + name + '(')
    opening = source.index('{', start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[:opening+1] + transform(source[opening+1:end-1]) + source[end-1:]

if ARGS.reuse_build:
    previous = ARGS.reuse_build.resolve()
    evidence = json.loads((previous / 'report.json').read_text())
    required = {'compile-compute-probe', 'control-query-equivalence', 'generate-shaders',
                'workstation-tests', 'performance-tests', 'native-build', 'intent'}
    passed = {step['name'] for step in evidence['steps'] if step['status'] == 0}
    assert required <= passed, 'Reused build did not complete required validation'
    # Only data actually used by the frozen renderer must match that build's
    # evidence. Exporter metadata, previews and unused GLBs are not runtime inputs.
    for relative in runtime_files():
        if relative in evidence['source_hashes'] and not relative.startswith(('Art/AudioWorkstation/GemButtons/', 'Art/AudioWorkstation/GemFadersMeters/')):
            assert hashlib.sha256((SOURCE_ASSETS / relative).read_bytes()).hexdigest() == evidence['source_hashes'][relative], ('Validated runtime input changed', relative)
    assert hashlib.sha256((ROOT / 'BuildSupport/Workstation/compute_probe.cpp').read_bytes()).hexdigest() == evidence['source_hashes']['BuildSupport/Workstation/compute_probe.cpp']
    for filename in ['compute-probe', 'wormwood-profile']:
        shutil.copy2(previous / filename, OUT / filename)
    REPORT['reused_validation_report'] = str(previous / 'report.json')
    REPORT['reused_passed_steps'] = sorted(required)
else:
    run('compile-compute-probe', ['xcrun', 'clang++', '-std=c++17', '-O2', '-I/opt/homebrew/include',
        'BuildSupport/Workstation/compute_probe.cpp', '-L/opt/homebrew/lib', '-lwgpu_native',
        '-Wl,-rpath,/opt/homebrew/lib', '-o', OUT / 'compute-probe'], 60)
    run('control-query-equivalence', ['python3', 'BuildSupport/Blender/check_control_acceleration.py'], 180)
    run('generate-shaders', ['python3', 'Procedures/Shaders/generate_workstation.py'], 30)
original_packet = (BASE / 'controls.bin').read_bytes()
current_packet = (SOURCE_ASSETS / 'Art/AudioWorkstation/DecorativeControls/controls.bin').read_bytes()
assert current_packet[:len(original_packet)] == original_packet, 'Original control packet changed'
REPORT['original_control_packet_byte_identical'] = True
REPORT['source_assets'] = str(SOURCE_ASSETS)
REPORT['source_hashes'] = fingerprint()
save()
if not ARGS.reuse_build:
    run('workstation-tests', ['cargo', 'test', '--release', '--offline', '--locked', '--lib', 'workstation_', '--', '--nocapture'], 300)
    run('performance-tests', ['cargo', 'test', '--release', '--offline', '--locked', '--lib', 'performance_', '--', '--nocapture'], 180)
    run('native-build', ['cargo', 'build', '--release', '--offline', '--locked', '--bin', 'wormwood'], 300)
    shutil.copy2(ROOT / 'target/release/wormwood', OUT / 'wormwood-profile')
    run('intent', [OUT / 'wormwood-profile', '--inspect-intent', '--json'], 30)
REPORT['executable_sha256'] = hashlib.sha256((OUT / 'wormwood-profile').read_bytes()).hexdigest()

CURRENT_ASSETS = frozen_assets('candidate-assets')
old_assets = frozen_assets('baseline-assets', baseline=True)
REPORT['baseline_scope'] = 'Same frozen current scene/material code; original control packet and a private false meter-band branch select the full original BVH. Other agents\' new scene assets are identical in both variants.'
save()
startup_env = os.environ.copy()
startup_env.update(WORMWOOD_ASSET_ROOT=str(CURRENT_ASSETS),
                   WORMWOOD_OUTPUT_ROOT=str(OUT / 'startup'), WORMWOOD_PERFORMANCE='0')
# Intent inspection checks declarations only. Actual startup also resolves the
# catalog against this frozen executable's compiled hooks and initializers.
run('runtime-startup', [OUT / 'wormwood-profile'], 25, startup_env)
frames = {}
for variant, assets in [('before', old_assets), ('after', CURRENT_ASSETS)]:
    dest = OUT / ('native-' + variant)
    dest.mkdir()
    env = os.environ.copy()
    env.update(WORMWOOD_ASSET_ROOT=str(assets), WORMWOOD_OUTPUT_ROOT=str(dest),
               WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA='0',
               WORMWOOD_WORKSTATION_GPU_TIMING='1', WORMWOOD_PERFORMANCE='0')
    run('native-' + variant, [OUT / 'wormwood-profile', '--workstation', 'WebGPU', '--frames', '48'], 150, env)
    verify_sources()
    frames[variant] = json.loads((dest / 'workstation-optical-frame.json').read_text())
    timing = json.loads((dest / 'workstation-gpu-timing-WebGPU.json').read_text())
    REPORT.setdefault('native_gpu_timing', {})[variant] = timing
    if timing.get('supported'):
        REPORT.setdefault('native_warm_gpu_ms', {})[variant] = statistics.median(timing['gpu_ms'][16:-1])
    frame_binary(frames[variant], OUT / (variant + '.bin'))
    save()

before = frames['before']['rows']
after = frames['after']['rows']
for i in range(12544):
    a, b = list(before[i]), list(after[i])
    if i in range(193, 212, 2):
        a[2] = b[2] = 0  # New optional acceleration pointer, not scene geometry.
    assert a == b, ('non-acceleration scene row changed', i, a, b)
assert before[45312:] == after[45312:], 'SSLE changed'
comparison = compare_images(OUT / 'native-before/triangle-WebGPU.ppm', OUT / 'native-after/triangle-WebGPU.ppm')
REPORT['native_capture_comparison'] = comparison
save()
assert comparison['mean_absolute_byte_difference'] < .1, comparison

for backend in ['WebGPU', 'Vulkan', 'OpenGL']:
    dest = OUT / ('benchmark-' + backend)
    dest.mkdir()
    env = os.environ.copy()
    env.update(WORMWOOD_ASSET_ROOT=str(CURRENT_ASSETS), WORMWOOD_OUTPUT_ROOT=str(dest),
               WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_PERFORMANCE='1')
    env.pop('WORMWOOD_WORKSTATION_GPU_TIMING', None)
    run('benchmark-' + backend, [OUT / 'wormwood-profile', '--benchmark-workstation', backend, '--frames', '8'], 150, env)
    verify_sources()
    report = json.loads((dest / ('workstation-benchmark-' + backend + '.json')).read_text())
    assert report['benchmark']['complete'], report
    gpu = report['gpu_measurement']
    assert isinstance(gpu.get('supported'), bool), ('Missing GPU capability report', gpu)
    if gpu['supported']:
        assert gpu.get('complete') is True, ('Incomplete measured GPU samples', gpu)
    REPORT.setdefault('benchmark_validation', {})[backend] = report
    save()
run('launcher-compile', ['xcrun', 'clang', '-fobjc-arc', '-framework', 'Cocoa',
    'BuildSupport/macOS/Launcher.m', '-o', OUT / 'MirrorLauncher'], 60)

current = (CURRENT_ASSETS / 'Procedures/Shaders/workstation.wgsl').read_text()
variants = {'before': (old_assets / 'Procedures/Shaders/workstation.wgsl').read_text(), 'after': current}
for name, bounds in [('top', (-.55, .65, .35, 1.)), ('bottom', (-.55, .65, -1., -.35)),
                     ('sides', None), ('floor', (-.5, .55, -.35, .35))]:
    if bounds:
        x0, x1, y0, y1 = bounds
        inside = f'screen.x>={x0}&&screen.x<={x1}&&screen.y>={y0}&&screen.y<={y1}'
    else:
        inside = 'screen.x< -0.55||screen.x>0.65'
    variants['region-' + name] = current.replace('fn workstationShade(', 'fn profileFullShade(') + f'''
fn workstationShade(screen:vec2<f32>)->vec4<f32>{{
    if({inside}){{return profileFullShade(screen);}}
    return vec4<f32>(0.0,0.0,0.0,1.0);
}}
'''
variants['diagnostic-no-controls'] = change_function(current, 'gemControlHit', lambda _: '\nreturn GemHit(farLimit,vec3<f32>(0.0),-1);\n')
variants['diagnostic-no-engraving'] = change_function(current, 'gemEngraving', lambda _: '\nreturn vec4<f32>(0.0,0.0,0.0,1.0);\n')

def scalar_priorities(body):
    body = body.replace('var pending:array<GemRay,16>;', 'var pending:array<GemRay,16>;var pendingPriorities:array<f32,16>;')
    body = body.replace('pending[candidate].priority', 'pendingPriorities[candidate]')
    body = body.replace('pending[strongest]=swapRay;', 'pending[strongest]=swapRay;pendingPriorities[strongest]=swapRay.priority;')
    for name in ['reflection', 'transmission']:
        body = body.replace(f'pending[count]={name};', f'pending[count]={name};pendingPriorities[count]={name}.priority;')
    return body
variants['candidate-scalar-priorities'] = change_function(current, 'gemTrace', scalar_priorities)

def counters(body):
    body = body.replace('var count:i32=0;', 'var count:i32=0;var counters:vec3<f32>=vec3<f32>(0.0);')
    marker = 'var environment:vec4<f32>='
    assert marker in body
    body = body.replace(marker, 'counters.x+=1.0;counters.z=max(counters.z,f32(count));if(current.medium>=4){counters.y+=1.0;}\n' + marker)
    assert 'return result;' in body
    return body.replace('return result;', 'return counters;')
variants['diagnostic-ray-counts'] = change_function(current, 'gemTrace', counters).replace(
    'return vec4<f32>(gemDisplay(result/samples),1.0);', 'return vec4<f32>(result/samples,1.0);')

for name, shader in variants.items():
    path = OUT / (name + '.wgsl')
    path.write_text(shader)
    frame = 'before' if name == 'before' else 'after'
    width, height = map(int, frames[frame]['rows'][8][:2])
    prefix = OUT / ('compute-' + name)
    run('compute-' + name, [OUT / 'compute-probe', path, OUT / (frame + '.bin'), width, height, prefix, 6], 120)
    verify_sources()
    result = json.loads(prefix.with_suffix('.json').read_text())
    REPORT.setdefault('compute', {})[name] = result
    if name in ['before', 'after', 'candidate-scalar-priorities']:
        comparison = compare_images(
            OUT / ('native-' + frame) / 'triangle-WebGPU.ppm', prefix.with_suffix('.ppm'))
        REPORT.setdefault('compute_capture_comparison', {})[name] = comparison
        result['visual_equivalence_gate_passed'] = comparison['mean_absolute_byte_difference'] < .1
        if not result['visual_equivalence_gate_passed']:
            result['candidate_status'] = 'Rejected for promotion: image difference exceeds fidelity gate.'
    if name == 'diagnostic-ray-counts':
        rows = list(struct.iter_unpack('<4f', prefix.with_suffix('.rgba32f').read_bytes()))
        REPORT['ray_statistics'] = {label: {'mean': statistics.mean(p[i] for p in rows), 'maximum_pixel_sample_average': max(p[i] for p in rows)}
            for i, label in enumerate(['events_per_sample', 'control_interior_events_per_sample', 'pending_at_event_start_per_sample'])}
        result['timing_interpretation'] = 'Counter-only shader permits radiance dead-code elimination; duration is not a production cost comparison.'
    save()

for mode in ['exact-facets', 'analytic-construction']:
    prefix = OUT / ('candidate-' + mode)
    run('generate-' + mode, ['python3', ROOT / 'BuildSupport/Workstation/analytic_controls.py',
        '--shader', CURRENT_ASSETS / 'Procedures/Shaders/workstation.wgsl', '--frame', OUT / 'after.bin',
        '--asset-root', CURRENT_ASSETS,
        '--output', prefix, '--mode', mode, '--rays', '256'], 60)
    width, height = map(int, frames['after']['rows'][8][:2])
    capture = OUT / ('compute-' + mode)
    run('compute-' + mode, [OUT / 'compute-probe', prefix.with_suffix('.wgsl'), prefix.with_suffix('.bin'),
        width, height, capture, 6], 120)
    verify_sources()
    result = json.loads(capture.with_suffix('.json').read_text())
    result['construction_validation'] = json.loads(prefix.with_suffix('.json').read_text())
    comparison = compare_images(OUT / 'compute-after.ppm', capture.with_suffix('.ppm'))
    result['capture_comparison'] = comparison
    result['visual_equivalence_gate_passed'] = comparison['mean_absolute_byte_difference'] < .1
    result['candidate_status'] = 'Private experiment; requires visual review before promotion.' if result['visual_equivalence_gate_passed'] else 'Rejected for promotion: image difference exceeds fidelity gate.'
    REPORT['compute'][mode] = result
    save()

print(json.dumps({'report': str(OUT / 'report.json'), 'native_gpu_ms': REPORT.get('native_warm_gpu_ms'),
    'compute_gpu_ms': {k: v['median_gpu_ms'] for k, v in REPORT['compute'].items()},
    'capture_comparison': REPORT['native_capture_comparison'], 'packaged': False}), flush=True)
