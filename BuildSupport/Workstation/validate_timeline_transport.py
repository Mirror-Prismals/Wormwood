#!/usr/bin/env python3
"""Finite staged timeline-cache validation; never installs or promotes an app."""
from pathlib import Path
import array
import datetime
import hashlib
import json
import math
import os
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
from release_approved_ruby import source_manifest, tree_hashes


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def comparison(first, second):
    images = []
    for path in (first, second):
        values = array.array('f')
        values.frombytes(path.read_bytes())
        if sys.byteorder != 'little':
            values.byteswap()
        if len(values) != 640 * 480 * 4 or not all(math.isfinite(v) for v in values):
            raise RuntimeError('Incomplete or nonfinite GPU image: ' + str(path))
        images.append(values)
    differences = [abs(a-b) for i, (a, b) in enumerate(zip(*images)) if i % 4 != 3]
    return {'max_rgb_error': max(differences),
            'rgb_rmse': math.sqrt(sum(v*v for v in differences) / len(differences)),
            'rgb_channels_over_0_00003': sum(v > 0.00003 for v in differences),
            'alpha_scope': 'Diagnostic cache coverage only; excluded from appearance comparison.',
            'candidate_alpha_sum': sum(images[1][3::4])}


def main():
    out = ROOT / 'output/checks' / ('ruby-timeline-native-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    installed = ROOT / 'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine'
    fixture = ROOT / 'output/checks/ruby-convex-20261031-205611-021702/fixtures/original.frame.bin'
    report = {'status': 'running', 'installed': False, 'steps': [],
              'scope': 'Same staged app and optical budgets; new flags explicitly off/on. '
                       'Headless timeline appearance first, then fullscreen native motion/idle with audio.',
              'earlier_interrupted_run': str(ROOT / 'output/checks/ruby-pixel-native-20261031-220752-273721/report.json')}
    before = None
    staged = None
    staged_before = None

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, argv, timeout=120, env=None):
        print(name, flush=True)
        log = out / (name + '.log')
        try:
            run_checked(list(map(str, argv)), cwd=ROOT, log=log, timeout=timeout, env=env)
        except BaseException:
            print(log.read_text(errors='replace')[-6500:] if log.is_file() else 'No step log', flush=True)
            raise
        report['steps'].append(name)
        save()

    def inspect(name, executable):
        env = {k: v for k, v in os.environ.items() if not k.startswith('WORMWOOD_')}
        env['WORMWOOD_ASSET_ROOT'] = str(ROOT)
        run(name, [executable, '--inspect-intent', '--json'], env=env)
        inspection = json.loads((out / (name + '.log')).read_text())
        report[name] = inspection.get('summary')
        if inspection.get('summary', {}).get('active_structural_mismatches') != 0:
            raise RuntimeError('Intent declarations have structural mismatches: ' + name)
        save()

    try:
        if shutil.disk_usage(ROOT).free < 1024**3:
            raise RuntimeError('Need 1 GiB free for staged app and diagnostic artifacts')
        before = source_manifest()
        (out / 'source-before.json').write_text(json.dumps(before, indent=2) + '\n')
        report['installed_engine_before'] = digest(installed)
        report['original_fixture_sha256'] = digest(fixture)
        inspect('inspect-intent-before', installed)
        run('test-harness', [sys.executable, 'BuildSupport/Workstation/test_profile_workstation_sustained.py'])
        for test_filter in ('workstation', 'timestamp_interval_tests', 'surface_status_tests',
                            'nondrawable_recovery_only_labels_observed_window_gates', 'benchmark_counter_window_tests'):
            run('test-' + test_filter, ['cargo', 'test', '--release', '--offline', '--locked',
                '--lib', test_filter, '--', '--test-threads=1'], 1200)
        deps = ROOT / 'target/release/deps'
        libraries = {name: max(deps.glob('lib' + name + '-*.rlib'), key=lambda p: p.stat().st_mtime)
                     for name in ('naga', 'serde_json')}
        exporter = out / 'export-timeline'
        run('compile-timeline-exporter', ['rustc', '--edition=2024', '-O',
            'BuildSupport/Workstation/export_ruby_timeline.rs',
            '--extern', 'naga=' + str(libraries['naga']),
            '--extern', 'serde_json=' + str(libraries['serde_json']),
            '-L', 'dependency=' + str(deps), '-o', exporter])
        probe = out / 'timeline-probe'
        run('compile-timeline-probe', ['xcrun', 'clang++', '-std=c++17', '-O2',
            '-I/opt/homebrew/include', 'BuildSupport/Workstation/ruby_timeline_probe.cpp',
            '-L/opt/homebrew/lib', '-lwgpu_native', '-Wl,-rpath,/opt/homebrew/lib', '-o', probe])
        for variant, flag in [('baseline', '0'), ('candidate', '1')]:
            env = {k: v for k, v in os.environ.items() if not k.startswith('WORMWOOD_')}
            env['WORMWOOD_WORKSTATION_PIXEL_TRANSPORT'] = flag
            run('export-' + variant, [exporter, ROOT / 'Procedures/Shaders/workstation.wgsl',
                fixture, ROOT, out / variant], env=env)
        if digest(out / 'baseline.frame.bin') != digest(out / 'candidate.frame.bin'):
            raise RuntimeError('A/B timeline fixture packets differ')
        report['timeline_fixture_sha256'] = digest(out / 'candidate.frame.bin')
        report['candidate_plan'] = json.loads((out / 'candidate.json').read_text())
        for label, variant, mode, resolve in [('baseline-warm', 'baseline', 'warm', 'resolve'),
                                      ('candidate-warm', 'candidate', 'warm', 'resolve'),
                                      ('candidate-cold', 'candidate', 'cold', 'resolve'),
                                      ('candidate-resolve-once', 'candidate', 'warm', 'resolve-once')]:
            run('gpu-' + label, [probe, out / (variant + '.wgsl'), out / (variant + '.frame.bin'),
                out / (variant + '.plan.bin'), 640, 480, out / label, 6, mode, resolve], 300)
            report[label] = json.loads((out / (label + '.json')).read_text())
            if report[label].get('nonfinite_output_channels') != 0:
                raise RuntimeError('GPU probe reported invalid output: ' + label)
        for label in ('candidate-warm', 'candidate-cold', 'candidate-resolve-once'):
            result = comparison(out / 'baseline-warm.rgba32f', out / (label + '.rgba32f'))
            report[label + '-appearance'] = result
            save()
            if result['max_rgb_error'] > 0.00003 or result['candidate_alpha_sum'] <= 0:
                raise RuntimeError('Timeline appearance or actual cache coverage gate failed: ' + label)
        for label in ('baseline-warm', 'candidate-warm'):
            run('image-' + label, ['/usr/bin/sips', '-s', 'format', 'png',
                out / (label + '.ppm'), '--out', out / (label + '.png')], 30)
        run('build-staged', [sys.executable, 'BuildSupport/macOS/build_app.py', '--output-dir', out / 'staged'], 1200)
        app = out / 'staged/MirrorPrismals.app'
        staged = app
        report['staged_app'] = str(app)
        run('verify-signing', ['codesign', '--verify', '--deep', '--strict', app], 60)
        inspect('inspect-intent-staged', app / 'Contents/MacOS/wormwood-engine')
        staged_before = tree_hashes(app)
        (out / 'staged-bundle-before.json').write_text(json.dumps(staged_before, indent=2) + '\n')
        if before != source_manifest():
            raise RuntimeError('Source changed during build; no native comparison claimed')
        common = [sys.executable, 'BuildSupport/Workstation/profile_workstation_sustained.py',
                  '--app', app, '--candidate', app, '--fullscreen', '--retina', '0',
                  '--frames', '120', '--warmup', '120', '--cycle', '32', '--repeats', '3',
                  '--max-attempts', '3', '--min-seconds', '30', '--audio',
                  '--conditions-note', 'Other user applications remain running; no exclusive hardware or thermal steady-state claim. Actual fullscreen dimensions recorded.']
        for flag in ('PIXEL_TRANSPORT', 'FLOOR_SAMPLE_CACHE', 'RESOLVE_REUSE'):
            common.extend(['--baseline-env', 'WORMWOOD_WORKSTATION_' + flag + '=0',
                           '--candidate-env', 'WORMWOOD_WORKSTATION_' + flag + '=1'])
        for scenario in ('motion', 'idle'):
            destination = out / (scenario + '-normal')
            run(scenario + '-normal', [*common, '--scenario', scenario, '--output', destination], 6000)
            subreport = json.loads((destination / 'report.json').read_text())
            report[scenario + '-normal'] = {'status': subreport['status'],
                'report': str(destination / 'report.json'), 'comparisons': subreport.get('comparisons', [])}
            save()
            if subreport['status'] != 'completed_graphics_and_native_audio':
                raise RuntimeError('Native comparison incomplete or unmatched: ' + scenario)
        report['status'] = 'completed_staged_diagnostics'
    except BaseException as error:
        report.update(status='failed_staged_diagnostics', error=str(error))
        raise
    finally:
        checks = []
        if before is not None:
            checks.append(('source_unchanged', lambda: before == source_manifest()))
        if 'installed_engine_before' in report:
            checks.append(('installed_engine_unchanged', lambda: report['installed_engine_before'] == digest(installed)))
        if 'original_fixture_sha256' in report:
            checks.append(('original_fixture_unchanged', lambda: report['original_fixture_sha256'] == digest(fixture)))
        if staged_before is not None:
            checks.append(('staged_bundle_unchanged', lambda: staged_before == tree_hashes(staged)))
        report['identity_check_errors'] = {}
        for key, check in checks:
            try:
                report[key] = check()
            except Exception as error:
                report[key] = False
                report['identity_check_errors'][key] = str(error)
        identity_failed = report['status'] == 'completed_staged_diagnostics' and (
            len(checks) != 4 or not all(report[key] for key, _ in checks))
        if identity_failed:
            report.update(status='failed_identity_check', error='Source, installed engine, original fixture or staged bundle changed or became unreadable')
        save()
        print(json.dumps({'status': report['status'], 'installed': False, 'report': str(out / 'report.json')}), flush=True)
        if identity_failed:
            raise RuntimeError(report['error'])


if __name__ == '__main__':
    main()
