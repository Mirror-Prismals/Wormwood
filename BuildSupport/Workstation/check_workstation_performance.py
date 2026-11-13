#!/usr/bin/env python3
"""Finite native comparison against a frozen copy of the installed local app."""
from pathlib import Path
import datetime
import json
import math
import os
import shutil
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport/macOS'))
from build_combined import (build_input, clean_env, digest, ppm_info, png_info,
                            read_json, require, run_checked, tail, test_count,
                            tree_hashes)


def statistics(values):
    values = sorted(float(value) for value in values)
    require(values and all(math.isfinite(v) and v >= 0 for v in values), 'Invalid timing samples')
    percentile = lambda p: values[round((len(values) - 1) * p)]
    return {'count': len(values), 'mean': sum(values) / len(values),
            'p50': percentile(.5), 'p95': percentile(.95), 'worst': values[-1]}


def benchmark_result(destination, measured):
    path = destination / 'workstation-benchmark-WebGPU.json'
    report = read_json(path)
    result, gpu = report['benchmark'], report['gpu_measurement']
    require(report.get('entities_remaining') == 0 and report.get('end_reason') == 'frame_limit',
            'Benchmark did not finish and release its Entities')
    require(result.get('complete') is True and result.get('measured_frames') == measured
            and result.get('requested_measured_frames') == measured
            and result.get('warmup_frames') == 4 and not result.get('resized_during_measurement'),
            'Incomplete or resized workstation benchmark')
    require(gpu.get('supported') is True and gpu.get('complete') is True
            and gpu.get('measured_samples') == measured and gpu.get('expected_samples') == measured,
            'GPU timestamps do not cover every measured frame')
    scope = str(gpu.get('scope', '')).lower()
    require('render pass' in scope and 'timestamp' in str(gpu.get('method', '')),
            'GPU timing does not use the corrected workstation render-pass interval')
    samples = result['samples']
    require(len(samples) == measured and all(s['dimensions'] == result['dimensions'] for s in samples),
            'Benchmark samples have inconsistent dimensions')
    return {'report': str(path), 'dimensions': result['dimensions'], 'measured_frames': measured,
            'frame_ms': statistics([s['frame_ms'] for s in samples]),
            'update_ms': statistics([s['update_ms'] for s in samples]),
            'render_present_ms': statistics([s['render_present_ms'] for s in samples]),
            'gpu': gpu, 'rendered_frames_per_second': result['rendered_frames_per_second']}


def controls_result(destination, backend):
    path = destination / f'workstation-controls-{backend}.json'
    report = read_json(path)
    require(report.get('frames') == 4 and report.get('end_reason') == 'frame_limit'
            and report.get('entities_remaining') == 0 and report.get('complete') is True
            and report.get('controls_settled') is True and report.get('controls_mixer') is True
            and report.get('mixer_controls_visible') is True, 'Incomplete native controls capture')
    require(all(report.get(k) is None for k in ('editor_error', 'arrangement_error', 'spatial_error')),
            'Native controls capture reports an editor error')
    return {'report': str(path), 'capture': ppm_info(destination / f'triangle-{backend}.ppm')}


def fallback_result(destination, kind):
    prefix = {'arrangement': 'arrangement', 'bubbles': 'bubble-piano-roll',
              'spheres': 'audio-spheres', 'custom': 'workstation-controls'}[kind]
    path = destination / f'{prefix}-WebGPU.json'
    value = read_json(path)
    require(value.get('frames') == 4 and value.get('end_reason') == 'frame_limit'
            and value.get('entities_remaining') == 0, f'Incomplete {kind} fallback capture')
    require(all(value.get(k) is None for k in ('editor_error', 'arrangement_error', 'spatial_error')),
            f'{kind} fallback reports a session error')
    rows = read_json(destination / 'workstation-optical-frame.json')['rows']
    require(len(rows) == 49408, f'{kind} optical packet differs')
    header = {'arrangement': 248, 'bubbles': 234, 'spheres': 240, 'custom': 245}[kind]
    require(rows[header][0] >= 1, f'{kind} feature was absent, so fallback was not exercised')
    return {'report': str(path), 'feature_count': rows[header][0],
            'capture': ppm_info(destination / 'triangle-WebGPU.ppm')}


def shader_selection(destination, selected):
    value = read_json(destination / 'workstation-shader-WebGPU.json')
    require(value.get('specialized_available') is True and value.get('error') is None
            and value.get('selected') == selected, f'Unexpected native shader selection: {value}')
    return value


def compare_captures(baseline, candidate, difference):
    first, second = ppm_info(baseline), ppm_info(candidate)
    require((first['width'], first['height']) == (second['width'], second['height']),
            'Baseline/candidate capture dimensions differ')
    changed, above_two, total_error, maximum = 0, 0, 0, 0
    with baseline.open('rb') as a, candidate.open('rb') as b, difference.open('wb') as output:
        for _ in range(3):
            header = a.readline(80)
            b.readline(80)
            output.write(header)
        while first_bytes := a.read(3 * 65536):
            second_bytes = b.read(len(first_bytes))
            require(len(first_bytes) == len(second_bytes), 'Truncated comparison capture')
            delta = [abs(x - y) for x, y in zip(first_bytes, second_bytes)]
            total_error += sum(delta)
            maximum = max(maximum, max(delta))
            changed += sum(max(delta[i:i+3]) > 0 for i in range(0, len(delta), 3))
            above_two += sum(max(delta[i:i+3]) > 2 for i in range(0, len(delta), 3))
            output.write(bytes(min(255, value * 8) for value in delta))
    pixels = first['width'] * first['height']
    return {'dimensions': [first['width'], first['height']], 'identical': changed == 0,
            'changed_pixels': changed, 'changed_percent': 100 * changed / pixels,
            'pixels_above_two_levels_percent': 100 * above_two / pixels,
            'mean_absolute_channel_error': total_error / (pixels * 3), 'maximum_channel_error': maximum,
            'baseline': first, 'candidate': second, 'difference_times_eight': str(difference),
            'visual_review': 'pending', 'threshold_enforced': False}


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / 'output/checks' / ('workstation-performance-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    installed = ROOT / 'output/macOS/MirrorPrismals.app'
    apps = {'baseline': out / 'baseline/MirrorPrismals.app',
            'candidate': out / 'candidate/MirrorPrismals.app'}
    report = {'status': 'running', 'output': str(out), 'checks': [], 'failures': [],
              'apps': {k: str(v) for k, v in apps.items()}, 'benchmarks': {}, 'captures': {},
              'installed': False, 'promoted': False, 'visual_review': 'pending',
              'scope': 'Matched authored workstation graphics; benchmark does not start playback. '
                       'Windowed 32-frame motion and fullscreen 8-frame reproduction exclude four warmup frames.',
              'hardware_exclusive': False,
              'live_session_note': 'User session PID 90284 was idle during the prior three-second '
                                   'preflight sample, with audio active. It is not stopped or controlled. '
                                   'Other activity can affect these sequential measurements.',
              'preflight_sample': str(ROOT / 'output/checks/workstation-performance-live-20261031/sample.txt')}
    before = None
    installed_before = None
    bundles = {}
    style_hashes = None
    style_fixture = out / 'fixtures/flat-ui'
    env = clean_env()
    env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(out / 'build-runtime'))

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def check(name, action):
        record = {'name': name, 'status': 'running'}
        report['checks'].append(record)
        save()
        start = time.monotonic()
        try:
            record['evidence'] = action()
            record['status'] = 'passed'
            return True
        except Exception as error:
            record.update(status='failed', error=f'{type(error).__name__}: {error}')
            report['failures'].append(name)
            print(f'{name}: {error}', flush=True)
            return False
        finally:
            record['seconds'] = round(time.monotonic() - start, 3)
            save()

    def run(name, argv, timeout, process_env=None, verify=None):
        command, log = [str(v) for v in argv], out / (name + '.log')
        def action():
            print(name, flush=True)
            try:
                run_checked(command, cwd=ROOT, log=log, timeout=timeout,
                            env=env if process_env is None else process_env, heartbeat=30)
                return {'argv': command, 'log': str(log), 'deadline_seconds': timeout,
                        'verification': verify(log) if verify else None}
            except Exception:
                print(tail(log), flush=True)
                raise
        return check(name, action)

    def manifest(label):
        require(run(label + '-git', ['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], 30),
                'Cannot enumerate source inputs')
        names = {os.fsdecode(p) for p in (out / (label + '-git.log')).read_bytes().split(b'\0') if p}
        result = {p: digest(ROOT / p) if (ROOT / p).is_file() else None
                  for p in sorted(names) if build_input(p)}
        require('Cargo.toml' in result and 'lib.rs' in result, 'Incorrect source manifest root')
        (out / (label + '.json')).write_text(json.dumps(result, indent=2) + '\n')
        return result

    def snapshot_bundle(label, path):
        require((path / 'Contents/MacOS/wormwood-engine').is_file(), 'Packaged engine missing')
        hashes = tree_hashes(path)
        (out / (label + '.json')).write_text(json.dumps(hashes, indent=2) + '\n')
        return hashes

    def case_environment(label, variant, fullscreen=False):
        destination = out / label
        destination.mkdir(exist_ok=False)
        native = clean_env()
        native.update(WORMWOOD_ASSET_ROOT=str(apps[variant] / 'Contents/Resources/EngineAssets'),
                      WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN=str(int(fullscreen)),
                      WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_WORKSTATION_SIZE='640x480',
                      WORMWOOD_PERFORMANCE='1')
        return destination, native

    def preview(name, ppm):
        if ppm.is_file():
            png = ppm.with_suffix('.png')
            run(name, ['/usr/bin/sips', '-s', 'format', 'png', ppm, '--out', png], 30,
                verify=lambda _: png_info(png))

    try:
        installed_before = snapshot_bundle('installed-before', installed)
        require(run('freeze-baseline', ['/usr/bin/ditto', installed, apps['baseline']], 180),
                'Cannot freeze installed baseline')
        bundles['baseline'] = snapshot_bundle('baseline-before', apps['baseline'])
        require(bundles['baseline'] == installed_before, 'Frozen baseline differs from installed app')
        # Author examples are not shipped in the normal app. Both versions load
        # one identical private copy through the supported style-package path.
        shutil.copytree(ROOT / 'Art/AudioWorkstation/StyleExamples/flat-ui', style_fixture)
        style_hashes = tree_hashes(style_fixture)
        (out / 'style-fixture.json').write_text(json.dumps(style_hashes, indent=2) + '\n')
        run('other-engine-processes', ['/bin/ps', '-axo', 'pid=,comm='], 15,
            verify=lambda log: {'observed': [line for line in log.read_text().splitlines()
                                if 'wormwood' in line.lower() or 'mirrorprismals' in line.lower()],
                                'action': 'none; existing sessions preserved'})
        require(run('generate-workstation', [sys.executable, 'Procedures/Shaders/generate_workstation.py'], 60),
                'Shader generation failed')
        before = manifest('source-before')
        for pattern in ('workstation', 'performance'):
            run('test-' + pattern, ['cargo', 'test', '--release', '--offline', '--locked', '--lib',
                pattern, '--', '--test-threads=1'], 1200, verify=test_count)
        built = run('build-candidate', [sys.executable, 'BuildSupport/macOS/build_app.py',
                    '--output-dir', out / 'candidate'], 1200)
        require(built, 'Candidate package failed; baseline preserved')
        bundles['candidate'] = snapshot_bundle('candidate-before', apps['candidate'])

        def candidate_assets():
            assets = apps['candidate'] / 'Contents/Resources/EngineAssets'
            mismatches = []
            for path in assets.rglob('*'):
                if path.is_file():
                    relative = path.relative_to(assets)
                    source = ROOT / ('BuildSupport/Licenses' if relative.parts[0] == 'ThirdPartyLicenses'
                                     else relative.parts[0]) / Path(*relative.parts[1:])
                    if not source.is_file() or digest(path) != digest(source):
                        mismatches.append(str(relative))
            require(not mismatches, f'Packaged candidate assets differ: {mismatches[:20]}')
            prefix = 'Contents/Resources/EngineAssets/Art/'
            authored = {v: {k: h for k, h in hashes.items() if k.startswith(prefix)}
                        for v, hashes in bundles.items()}
            changed = sorted(k for k in authored['baseline'].keys() | authored['candidate'].keys()
                             if authored['baseline'].get(k) != authored['candidate'].get(k))
            report['authored_art_changes'] = changed
            require(not changed, 'Authored Art assets changed between baseline and candidate')
            return {'checkout_assets_match': True, 'authored_art_identical': True}
        check('candidate-assets', candidate_assets)
        for variant, app in apps.items():
            run('codesign-' + variant, ['/usr/bin/codesign', '--verify', '--deep', '--strict', app], 60)
        destination, native = case_environment('candidate-intent', 'candidate')
        # The inspector checks source bindings, which deliberately are not
        # shipped in a runtime app bundle. Inspect the checkout used to build it.
        native['WORMWOOD_ASSET_ROOT'] = str(ROOT)
        def intent(log):
            value = read_json(log)['summary']
            require(value.get('active_structural_mismatches') == 0, f'Intent mismatch: {value}')
            return value
        run('candidate-intent', [apps['candidate'] / 'Contents/MacOS/wormwood-engine',
            '--inspect-intent', '--json'], 180, native, intent)

        for scenario, measured, fullscreen in (('windowed', 32, False), ('fullscreen', 8, True)):
            report['benchmarks'][scenario] = {}
            for variant, app in [*apps.items(), ('baseline-repeat', apps['baseline'])]:
                label = f'{scenario}-{variant}'
                destination, native = case_environment(label, 'baseline' if variant == 'baseline-repeat' else variant, fullscreen)
                def verify(_, d=destination, count=measured, s=scenario, v=variant):
                    result = benchmark_result(d, count)
                    report['benchmarks'][s][v] = result
                    return result
                run(label, [app / 'Contents/MacOS/wormwood-engine', '--benchmark-workstation',
                    'WebGPU', '--frames', str(measured)], 180, native, verify)
            def compare(s=scenario):
                pair = report['benchmarks'][s]
                require(set(pair) == {'baseline', 'candidate', 'baseline-repeat'}, f'{s} comparison missing a complete run')
                first, second = pair['baseline'], pair['candidate']
                repeated = pair['baseline-repeat']
                require(first['dimensions'] == second['dimensions'], 'Benchmark resolutions differ')
                require(first['dimensions'] == repeated['dimensions'], 'Repeated benchmark resolution differs')
                baseline_ms, candidate_ms = first['frame_ms']['mean'], second['frame_ms']['mean']
                require(baseline_ms > 0 and candidate_ms > 0, 'Zero frame duration')
                value = {'dimensions': first['dimensions'], 'measured_frames_each': first['measured_frames'],
                         'baseline_frame_ms_mean': baseline_ms, 'candidate_frame_ms_mean': candidate_ms,
                         'baseline_frame_ms_p95': first['frame_ms']['p95'],
                         'candidate_frame_ms_p95': second['frame_ms']['p95'],
                         'frame_time_reduction_percent': 100 * (1 - candidate_ms / baseline_ms),
                         'speedup_ratio': baseline_ms / candidate_ms,
                         'baseline_gpu_ms_mean': first['gpu']['gpu_ms_mean'],
                         'candidate_gpu_ms_mean': second['gpu']['gpu_ms_mean'],
                         'baseline_repeat_frame_ms_mean': repeated['frame_ms']['mean'],
                         'baseline_drift_percent': 100 * (repeated['frame_ms']['mean'] / baseline_ms - 1),
                         'speedup_against_repeated_baseline': repeated['frame_ms']['mean'] / candidate_ms,
                         'minimum_speedup_required': False, 'exclusive_hardware': False}
                report.setdefault('summary', {})[s] = value
                return value
            check('compare-' + scenario, compare)

        for backend in ('WebGPU', 'Vulkan', 'OpenGL'):
            paths = {}
            report['captures'][backend] = {}
            for variant, app in apps.items():
                label = f'controls-{backend}-{variant}'
                destination, native = case_environment(label, variant)
                def verify(_, d=destination, api=backend, v=variant):
                    result = controls_result(d, api)
                    if api == 'WebGPU' and v == 'candidate':
                        result['shader_selection'] = shader_selection(d, 'controls_only')
                    report['captures'][api][v] = result
                    return result
                if run(label, [app / 'Contents/MacOS/wormwood-engine', '--check-workstation-controls', backend],
                       180, native, verify):
                    paths[variant] = destination / f'triangle-{backend}.ppm'
                    preview(label + '-png', paths[variant])
            difference = out / f'controls-{backend}-difference-x8.ppm'
            def compare(api=backend, captures=paths, target=difference):
                require(set(captures) == {'baseline', 'candidate'}, 'Controls comparison missing a valid capture')
                value = compare_captures(captures['baseline'], captures['candidate'], target)
                report['captures'][api]['comparison'] = value
                return value
            if check('compare-controls-' + backend, compare):
                preview('difference-' + backend + '-png', difference)

        # Each family excluded from the fast program must still render through
        # the canonical program when it is actually present in a native frame.
        for kind, flag in (('arrangement', '--check-arrangement'),
                           ('bubbles', '--check-bubble-notes'),
                           ('spheres', '--check-audio-spheres'),
                           ('custom', '--check-workstation-controls')):
            paths = {}
            report.setdefault('fallback_captures', {})[kind] = {}
            for variant, app in apps.items():
                label = f'fallback-{kind}-{variant}'
                destination, native = case_environment(label, variant)
                if kind == 'custom':
                    native['WORMWOOD_STYLE'] = str(style_fixture)
                def verify(_, d=destination, k=kind, v=variant):
                    result = fallback_result(d, k)
                    if v == 'candidate':
                        result['shader_selection'] = shader_selection(d, 'full')
                    report['fallback_captures'][k][v] = result
                    return result
                if run(label, [app / 'Contents/MacOS/wormwood-engine', flag, 'WebGPU'],
                       180, native, verify):
                    paths[variant] = destination / 'triangle-WebGPU.ppm'
                    preview(label + '-png', paths[variant])
            difference = out / f'fallback-{kind}-difference-x8.ppm'
            def compare(k=kind, captures=paths, target=difference):
                require(set(captures) == {'baseline', 'candidate'}, 'Fallback capture pair incomplete')
                value = compare_captures(captures['baseline'], captures['candidate'], target)
                report['fallback_captures'][k]['comparison'] = value
                return value
            if check('compare-fallback-' + kind, compare):
                preview('difference-fallback-' + kind + '-png', difference)
    except Exception as error:
        report['failures'].append('workflow')
        report['workflow_error'] = f'{type(error).__name__}: {error}'
    finally:
        if before is not None:
            def source_unchanged():
                after = manifest('source-after')
                changed = sorted(k for k in before.keys() | after.keys() if before.get(k) != after.get(k))
                report['sources_changed_during_run'] = changed
                require(not changed, f'Source changed during comparison: {changed[:30]}')
                return {'changed': []}
            check('source-revision-unchanged', source_unchanged)
        for variant, hashes in bundles.items():
            def bundle_unchanged(v=variant, expected=hashes):
                after = snapshot_bundle(v + '-after', apps[v])
                changed = sorted(k for k in expected.keys() | after.keys() if expected.get(k) != after.get(k))
                require(not changed, f'{v} bundle mutated during native checks: {changed[:20]}')
                return {'changed': []}
            check(variant + '-bundle-unchanged', bundle_unchanged)
        if installed_before is not None:
            def installed_unchanged():
                require(snapshot_bundle('installed-after', installed) == installed_before,
                        'Installed app changed during comparison')
                return {'unchanged': True}
            check('installed-app-unchanged', installed_unchanged)
        if style_hashes is not None:
            check('style-fixture-unchanged', lambda: require(tree_hashes(style_fixture) == style_hashes,
                  'Shared custom style fixture changed during native comparison'))
        report['status'] = 'failed_evidence_retained' if report['failures'] else 'passed_visual_review_pending'
        report['ready_for_promotion'] = False
        save()
    print(json.dumps({'report': str(out / 'report.json'), 'candidate': str(apps['candidate']),
                      'status': report['status'], 'failures': report['failures'],
                      'summary': report.get('summary', {}), 'promoted': False}), flush=True)
    return 1 if report['failures'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
