#!/usr/bin/env python3
"""Finite staged validation. Never installs, promotes, uploads, or edits source."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
from release_approved_ruby import source_manifest


def main():
    out = ROOT / 'output/checks' / ('ruby-pixel-native-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    report = {'status': 'running', 'installed': False, 'steps': [],
              'scope': 'Staged local validation only; existing playable application retained.',
              'prototype_evidence': str(ROOT / 'output/checks/ruby-pixel-20261031-215109')}
    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    def run(name, argv, timeout=120, env=None):
        print(name, flush=True)
        path = out / (name + '.log')
        try:
            run_checked([str(v) for v in argv], cwd=ROOT, log=path, timeout=timeout, env=env)
        except BaseException:
            print(path.read_text(errors='replace')[-6500:], flush=True)
            raise
        report['steps'].append(name)
        save()
    def file_hash(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()
    try:
        before = source_manifest()
        (out / 'source-before.json').write_text(json.dumps(before, indent=2))
        installed = ROOT / 'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine'
        report['installed_engine_before'] = file_hash(installed)
        run('inspect-intent', [installed, '--inspect-intent'])
        run('test-harness', [sys.executable, 'BuildSupport/Workstation/test_profile_workstation_sustained.py'])
        run('test-workstation', ['cargo', 'test', '--release', '--offline', '--locked', '--lib', 'workstation', '--', '--test-threads=1'], 1200)
        run('build-staged', [sys.executable, 'BuildSupport/macOS/build_app.py', '--output-dir', out / 'staged'], 1200)
        app = out / 'staged/MirrorPrismals.app'
        report['staged_app'] = str(app)
        run('verify-signing', ['codesign', '--verify', '--deep', '--strict', app], 60)
        if before != source_manifest():
            raise RuntimeError('Sources changed during the build; staged output is not a stable comparison.')
        captures = []
        for name, flag in [('baseline', '0'), ('candidate', '1')]:
            destination = out / ('controls-' + name)
            destination.mkdir()
            env = {k: v for k, v in os.environ.items() if not k.startswith('WORMWOOD_')}
            env.update(WORMWOOD_ASSET_ROOT=str(app / 'Contents/Resources/EngineAssets'),
                       WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN='0',
                       WORMWOOD_WORKSTATION_SIZE='640x480', WORMWOOD_WORKSTATION_RETINA='0',
                       WORMWOOD_WORKSTATION_PIXEL_TRANSPORT=flag, WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE=flag)
            run('controls-' + name, [app / 'Contents/MacOS/wormwood-engine', '--check-workstation-controls', 'WebGPU'], 180, env)
            result = json.loads((destination / 'workstation-controls-WebGPU.json').read_text())
            if result.get('complete') is not True:
                raise RuntimeError('Incomplete native controls run: ' + name)
            image = (destination / 'triangle-WebGPU.ppm').read_bytes()
            magic, size, maximum, pixels = image.split(b'\n', 3)
            if magic != b'P6' or maximum != b'255':
                raise RuntimeError('Unexpected capture encoding')
            captures.append((size, pixels))
            run('image-' + name, ['/usr/bin/sips', '-s', 'format', 'png', destination / 'triangle-WebGPU.ppm', '--out', out / ('controls-' + name + '.png')], 30)
        if captures[0][0] != captures[1][0] or len(captures[0][1]) != len(captures[1][1]):
            raise RuntimeError('Native capture dimensions changed')
        errors = [abs(a-b) for a,b in zip(captures[0][1], captures[1][1])]
        report['native_controls_comparison'] = {'dimensions': captures[0][0].decode(),
            'max_channel_error_255': max(errors),
            'rms_channel_error_255': (sum(v*v for v in errors)/len(errors))**0.5,
            'channels_over_one_code': sum(v>1 for v in errors),
            'channels_changed': sum(v>0 for v in errors),
            'interpretation': 'Diagnostic comparison only; differing appearance does not authorize promotion.'}
        save()
        common = [sys.executable, 'BuildSupport/Workstation/profile_workstation_sustained.py',
                  '--app', app, '--candidate', app,
                  '--baseline-env', 'WORMWOOD_WORKSTATION_PIXEL_TRANSPORT=0',
                  '--baseline-env', 'WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE=0',
                  '--candidate-env', 'WORMWOOD_WORKSTATION_PIXEL_TRANSPORT=1',
                  '--candidate-env', 'WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE=1',
                  '--frames', '120', '--warmup', '120', '--cycle', '32', '--repeats', '3',
                  '--min-seconds', '30', '--audio',
                  '--conditions-note', 'Other user applications remain running; no thermal steady-state claim. Current normal workstation default is non-Retina; fullscreen uses actual primary monitor mode.']
        for name, scenario, size_args in [
                ('motion-diagnostic', 'motion', ['--size', '640x480']),
                ('motion-normal', 'motion', ['--fullscreen']),
                ('idle-normal', 'idle', ['--fullscreen'])]:
            destination = out / name
            run(name, [*common, '--scenario', scenario, *size_args, '--output', destination], 2100)
            subreport = json.loads((destination / 'report.json').read_text())
            report[name] = {'status': subreport['status'], 'report': str(destination / 'report.json'),
                            'comparisons': subreport.get('comparisons', [])}
            save()
        report['source_unchanged'] = before == source_manifest()
        report['installed_engine_unchanged'] = report['installed_engine_before'] == file_hash(installed)
        if not report['source_unchanged'] or not report['installed_engine_unchanged']:
            raise RuntimeError('Source or existing app changed during validation; no promotion performed.')
        report['status'] = 'completed_staged_diagnostics'
    except BaseException as error:
        report.update(status='failed_staged_diagnostics', error=str(error))
        raise
    finally:
        save()
        print(json.dumps({'status': report['status'], 'installed': False, 'report': str(out / 'report.json')}), flush=True)


if __name__ == '__main__':
    main()
