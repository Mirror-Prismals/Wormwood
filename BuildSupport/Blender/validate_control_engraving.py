"""Finite Blender generation, focused native validation and SSLE captures.

Keeps a private executable/assets pair for rendering. No upload or app launch;
captures require visual review before packaging the Mac launcher.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--reuse-assets', action='store_true')
parser.add_argument('--art-only', action='store_true',
                    help='Render the authored models without building shared in-progress code')
parser.add_argument('--reuse-build', type=Path,
                    help='Resume captures from a hash-verified successful build and frozen assets')
parser.add_argument('--startup-only', action='store_true',
                    help='With --reuse-build, verify corrected startup resources without rendering')
args = parser.parse_args()
if args.reuse_build and args.art_only:
    parser.error('--reuse-build and --art-only are separate validation modes')
if args.startup_only and not args.reuse_build:
    parser.error('--startup-only requires a previously validated --reuse-build')
OUT = ROOT / 'output/checks' / ('control-engraving-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
report = {'output': str(OUT), 'steps': [], 'packaged': False, 'uploaded': False,
          'visual_review': 'pending'}


def save():
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')


def run(name, argv, timeout=180, env=None):
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


snapshot = OUT / 'assets'
executable = OUT / 'wormwood-engraving'
previous = None
if args.reuse_build:
    previous_dir = args.reuse_build.resolve()
    previous = json.loads((previous_dir / 'report.json').read_text())
    validation_steps = previous['steps'] + previous.get('reused_validation_steps', [])
    passed = {step['name'] for step in validation_steps if step['status'] == 0}
    assert {'workstation-checks', 'native-build'} <= passed, 'Build and tests must have passed'
    assert hashlib.sha256((previous_dir / 'wormwood-engraving').read_bytes()).hexdigest() == previous['executable_sha256']
    for relative, digest in previous['runtime_sha256'].items():
        assert hashlib.sha256((previous_dir / 'assets' / relative).read_bytes()).hexdigest() == digest, ('Frozen input changed', relative)
    shutil.copytree(previous_dir / 'assets', snapshot)
    shutil.copy2(previous_dir / 'wormwood-engraving', executable)
    report['reused_build'] = str(previous_dir)
    report['reused_validation_steps'] = [step for step in validation_steps
                                       if step['name'] in {'workstation-checks', 'native-build'}]
    report['executable_sha256'] = previous['executable_sha256']
    art = snapshot / 'Art/AudioWorkstation/ControlEngraving'
else:
    art = ROOT / 'Art/AudioWorkstation/ControlEngraving'
if not args.reuse_assets and not previous:
    run('blender-control-engraving', ['/Applications/Blender.app/Contents/MacOS/Blender',
        '--background', '--factory-startup', '--python-exit-code', '1',
        '--python', 'BuildSupport/Blender/make_control_engraving.py'], 300)
manifest = json.loads((art / 'control_engraving.json').read_text())
checks = previous['blender_checks'] if previous else json.loads((art / 'control_engraving_checks.json').read_text())
assert checks['source_kits_unchanged'] and checks['patterns'] == 5 and checks['assets'] == 6
assert len(checks['embedding_checks']) == 6
assert all(check['all_vertices_embedded'] for check in checks['embedding_checks'])
assert hashlib.sha256((art / manifest['density_binary']).read_bytes()).hexdigest() == manifest['density_sha256']
if not previous:
    for asset in manifest['assets']:
        assert hashlib.sha256((art / asset['glb']).read_bytes()).hexdigest() == asset['glb_sha256']
        original = ROOT / 'Art/AudioWorkstation' / asset['source_glb']
        assert hashlib.sha256(original.read_bytes()).hexdigest() == asset['source_glb_sha256']
report['blender_checks'] = checks
save()
if args.art_only:
    report['previews'] = {}
    for stem in ['GemButtons', 'GemFadersMeters']:
        preview = OUT / (stem + '-engraved.png')
        expression = (
            "import bpy; s=bpy.context.scene; s.render.engine='CYCLES'; "
            "s.cycles.time_limit=60; "
            "s.render.resolution_percentage=min(s.render.resolution_percentage, "
            "max(1, int(1280*100/max(s.render.resolution_x,s.render.resolution_y)))); "
            f"s.render.filepath={str(preview)!r}; bpy.ops.render.render(write_still=True)"
        )
        run('preview-' + stem, ['/Applications/Blender.app/Contents/MacOS/Blender',
            '--background', art / (stem + '_engraved.blend'), '--python-exit-code', '1',
            '--python-expr', expression], 180)
        assert preview.is_file() and preview.stat().st_size > 32, 'Blender preview missing'
        report['previews'][stem] = str(preview)
        save()
    report['ready_for_art_review'] = True
    report['native_validation'] = 'pending; shared source integration is still in progress'
    save()
    print(json.dumps({'report': str(OUT / 'report.json'), 'previews': report['previews'],
        'packaged': False, 'visual_review': 'pending'}), flush=True)
    raise SystemExit(0)
if not previous:
    run('pack-engraved-controls', ['python3', 'BuildSupport/Blender/export_workstation_controls.py'], 90)
    run('generate-shaders', ['python3', 'Procedures/Shaders/generate_workstation.py'], 20)

# Copy the startup metadata and runtime inputs as one set before the build,
# including new appearance extensions; no live asset symlinks are used.
patterns = ['Project.json', 'Systems/*.json', 'BaseSystem/registry.json',
    'BaseSystem/circuit_breaker.json', 'Procedures/**/*', 'SimulationWorkshop/**/*',
    'Art/AudioWorkstation/GemPanels/*.glb', 'Art/AudioWorkstation/GemPanels/GemPanels_runtime.json',
    'Art/AudioWorkstation/GemPanels/GemPanels_AgX.bin',
    'Art/AudioWorkstation/DecorativeControls/controls.*',
    'Art/AudioWorkstation/LaserEngraving/engraving.json', 'Art/AudioWorkstation/LaserEngraving/*.bin',
    'Art/AudioWorkstation/InputMonitor/runtime.*', 'Art/AudioWorkstation/EdisonHardware/runtime.*',
    'Art/AudioWorkstation/EdisonHardware/connected.*', 'Art/AudioWorkstation/Timeline/runtime.*',
    'Art/AudioWorkstation/BubbleNotes/BubbleNotes_runtime.*',
    'Art/AudioWorkstation/ControlEngraving/control_engraving.json',
    'Art/AudioWorkstation/ControlEngraving/control_engraving.bin']
if previous:
    # Supplement omitted startup resources without replacing any frozen shader,
    # contract, catalog or appearance input with a concurrent checkout revision.
    patterns = ['Procedures/Assets/**/*', 'Procedures/chuck/**/*', 'SimulationWorkshop/**/*']
files = sorted({p for pattern in patterns for p in ROOT.glob(pattern)
                if p.is_file() and '__pycache__' not in p.parts})
report['supplemental_assets'] = []
for path in files:
    relative = path.relative_to(ROOT)
    target = snapshot / relative
    if previous and target.exists():
        continue
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(path.read_bytes())
    if previous:
        report['supplemental_assets'].append(str(relative))
report['runtime_sha256'] = {str(p.relative_to(snapshot)): hashlib.sha256(p.read_bytes()).hexdigest()
                            for p in sorted(snapshot.rglob('*')) if p.is_file()}
save()
if not previous:
    run('workstation-checks', ['cargo', 'test', '--release', '--offline', '--locked', '--lib',
        'workstation_', '--', '--nocapture'], 360)
    run('native-build', ['cargo', 'build', '--release', '--offline', '--locked', '--bin', 'wormwood'], 300)
    build_target = Path(os.environ.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
    if not build_target.is_absolute():
        build_target = ROOT / build_target
    shutil.copy2(build_target / 'release/wormwood', executable)
    report['executable_sha256'] = hashlib.sha256(executable.read_bytes()).hexdigest()
env = os.environ.copy()
env.pop('WORMWOOD_WORKSTATION_GPU_TIMING', None)
env.pop('WORMWOOD_STYLE', None)
env.update(WORMWOOD_ASSET_ROOT=str(snapshot), WORMWOOD_OUTPUT_ROOT=str(OUT / 'startup'),
           WORMWOOD_PERFORMANCE='0', WORMWOOD_FULLSCREEN='0')
run('runtime-startup', [executable], 25, env)
if args.startup_only:
    report['startup_verified'] = True
    save()
    print(json.dumps({'report': str(OUT / 'report.json'), 'startup_verified': True,
                     'captures': 'pending'}), flush=True)
    raise SystemExit(0)

for backend, retina, frames in [('WebGPU', '0', 32), ('WebGPU', '1', 8),
                                ('Vulkan', '0', 8), ('OpenGL', '0', 8)]:
    name = backend + ('-Retina' if retina == '1' else '')
    dest = OUT / name
    dest.mkdir()
    env.update(WORMWOOD_OUTPUT_ROOT=str(dest), WORMWOOD_WORKSTATION_RETINA=retina)
    run('capture-' + name, [executable, '--workstation', backend, '--frames', frames], 150, env)
    frame = json.loads((dest / 'workstation-optical-frame.json').read_text())
    assert frame['rows'][244][1] == manifest['density_rows'], 'Engraving density header missing'
    capture = dest / ('triangle-' + backend + '.ppm')
    assert capture.is_file() and capture.stat().st_size > 32, 'Native capture missing'
    run('png-' + name, ['sips', '-s', 'format', 'png', capture, '--out', capture.with_suffix('.png')], 20)
    assert capture.with_suffix('.png').is_file(), 'Converted capture missing'
    report.setdefault('captures', {})[name] = {'dimensions': frame['rows'][8][:2],
        'png': str(dest / ('triangle-' + backend + '.png'))}
    save()

# Revalidate the corrected asynchronous pass timestamps through the public
# deterministic benchmark. GPU samples remain separate from wall/frame times.
dest = OUT / 'benchmark-WebGPU'
dest.mkdir()
env.update(WORMWOOD_OUTPUT_ROOT=str(dest), WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_PERFORMANCE='1')
run('benchmark-WebGPU', [executable, '--benchmark-workstation', 'WebGPU', '--frames', '8'], 150, env)
benchmark = json.loads((dest / 'workstation-benchmark-WebGPU.json').read_text())
assert benchmark['benchmark']['complete']
assert isinstance(benchmark['gpu_measurement']['supported'], bool), 'GPU capability report missing'
if benchmark['gpu_measurement']['supported']:
    assert benchmark['gpu_measurement']['complete']
report['benchmark'] = benchmark
run('launcher-compile', ['xcrun', 'clang', '-fobjc-arc', '-framework', 'Cocoa',
    'BuildSupport/macOS/Launcher.m', '-o', OUT / 'MirrorLauncher'], 60)
report['ready_for_visual_review'] = True
save()
print(json.dumps({'report': str(OUT / 'report.json'), 'captures': report['captures'],
    'packaged': False, 'visual_review': 'pending'}), flush=True)
