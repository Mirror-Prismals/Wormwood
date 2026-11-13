#!/usr/bin/env python3
"""Finite all-ruby cache validation, including rebuild cost and native rendering.

Run via Codex Process Jobs. Stages a candidate app without changing the installed
app; optical appearance and performance must be reviewed before promotion.
"""
from pathlib import Path
import array
import ast
import datetime
import hashlib
import json
import os
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked


def main():
    out = ROOT/'output/checks'/('ruby-cache-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    print('Evidence: '+str(out), flush=True)
    report = {'status': 'running', 'installed': False, 'steps': [], 'scope': 'WebGPU primary ruby panels and control components, with canonical fallback'}

    def save():
        (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')

    def run(name, command, timeout=180, env=None):
        print(name, flush=True)
        log = out/(name+'.log')
        try:
            run_checked([str(v) for v in command], cwd=ROOT, log=log, timeout=timeout, env=env)
        except BaseException:
            print(log.read_text(errors='replace')[-7000:], flush=True)
            raise
        report['steps'].append(name)
        save()

    try:
        if shutil.disk_usage(ROOT).free < 400*1024**2:
            raise RuntimeError('Need 400 MiB free for the private app and GPU validation')
        run('inspect-intent', [ROOT/'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine', '--inspect-intent'])
        run('generate-shaders', [sys.executable, 'Procedures/Shaders/generate_workstation.py'])
        run('validate-vulkan', ['glslangValidator', '-V', 'Procedures/Shaders/workstation.frag', '-o', out/'workstation.spv'])
        run('test-workstation', ['cargo', 'test', '--release', '--offline', '--locked', '--lib', 'workstation', '--', '--test-threads=1'], 1200)
        deps = ROOT/'target/release/deps'
        naga = max(deps.glob('libnaga-*.rlib'), key=lambda p: p.stat().st_mtime)
        serde_json = max(deps.glob('libserde_json-*.rlib'), key=lambda p: p.stat().st_mtime)
        exporter = out/'export-ruby-cache'
        run('compile-exporter', ['rustc', '--edition=2024', '-O', 'BuildSupport/Workstation/export_ruby_cache.rs',
            '--extern', 'naga='+str(naga), '--extern', 'serde_json='+str(serde_json),
            '-L', 'dependency='+str(deps), '-o', exporter])
        probe = out/'ruby-cache-probe'
        run('compile-probe', ['xcrun', 'clang++', '-std=c++17', '-O2', '-I/opt/homebrew/include',
            'BuildSupport/Workstation/ruby_cache_probe.cpp', '-L/opt/homebrew/lib', '-lwgpu_native',
            '-Wl,-rpath,/opt/homebrew/lib', '-o', probe])
        fixture = ROOT/'output/checks/optics-rethink-20261031-061827-752714/inputs/controls.bin'
        frame_hash = hashlib.file_digest(fixture.open('rb'), 'sha256').hexdigest()
        run('export-live-plan', [exporter, ROOT/'Procedures/Shaders/workstation.wgsl', fixture, out/'response'])
        report['plan'] = json.loads((out/'response.json').read_text())
        for mode, suffix in [('baseline', 'baseline'), ('warm', 'warm'), ('cold', 'cold'), ('baseline', 'baseline-repeat')]:
            run('gpu-'+suffix, [probe, out/'response.wgsl', fixture, out/'response.plan.bin',
                640, 480, out/suffix, 6, mode], 300)
            report[suffix] = json.loads((out/(suffix+'.json')).read_text())
            if report[suffix].get('nonfinite_output_channels', 0):
                raise RuntimeError('Nonfinite GPU output: '+suffix)
        for suffix in ['baseline', 'warm', 'cold']:
            run('image-'+suffix, ['/usr/bin/sips', '-s', 'format', 'png', out/(suffix+'.ppm'), '--out', out/(suffix+'.png')])
        pixels = array.array('f')
        pixels.frombytes((out/'warm.rgba32f').read_bytes())
        if sys.byteorder != 'little': pixels.byteswap()
        accepted = sum(pixels[3::4])
        report['accepted_primary_samples'] = accepted
        report['primary_samples'] = 640*480*2
        if accepted <= 8107:
            raise RuntimeError('Broader cache did not exceed the rejected four-cap coverage')
        report['warm_speedup'] = report['baseline']['median_gpu_ms']/report['warm']['median_gpu_ms']
        report['cold_speedup'] = report['baseline']['median_gpu_ms']/report['cold']['median_gpu_ms']
        report['baseline_drift_percent'] = 100*(report['baseline-repeat']['median_gpu_ms']/report['baseline']['median_gpu_ms']-1)
        if frame_hash != hashlib.file_digest(fixture.open('rb'), 'sha256').hexdigest():
            raise RuntimeError('Reference packet changed during validation')
        run('build-private-app', [sys.executable, 'BuildSupport/macOS/build_app.py', '--output-dir', out/'candidate'], 1200)
        app = out/'candidate/MirrorPrismals.app'
        report['candidate_app'] = str(app)
        for mode in ['baseline', 'response']:
            destination = out/('native-'+mode)
            destination.mkdir()
            env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
            env.update(WORMWOOD_ASSET_ROOT=str(app/'Contents/Resources/EngineAssets'),
                WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN='0',
                WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_WORKSTATION_SIZE='640x480',
                WORMWOOD_PERFORMANCE='1', WORMWOOD_RUBY_RESPONSE='0' if mode=='baseline' else '1')
            run('native-'+mode, [app/'Contents/MacOS/wormwood-engine', '--benchmark-workstation', 'WebGPU', '--frames', '12'], 240, env)
            report['native-'+mode] = {'output': str(destination)}
        selection = out/'native-response/workstation-shader-WebGPU.json'
        if not selection.is_file():
            raise RuntimeError('Native cache selection evidence missing')
        report['native_selection'] = json.loads(selection.read_text())
        if report['native_selection'].get('selected') != 'ruby_response':
            raise RuntimeError('Native app did not select the ruby response pipeline')
        report['status'] = 'validated_pending_visual_and_performance_review'
        save()
        print(json.dumps({'status': report['status'], 'report': str(out/'report.json'),
            'warm_speedup': report['warm_speedup'], 'cold_speedup': report['cold_speedup'],
            'accepted_primary_samples': accepted}), flush=True)
    except BaseException as exc:
        report['status'] = 'failed'
        report['error'] = str(exc)
        save()
        raise


if __name__ == '__main__':
    main()
