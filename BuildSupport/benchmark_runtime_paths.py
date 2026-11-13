"""Compare frozen runtime snapshots with the existing workstation/mining benchmarks.

The source directories must be private copies made before starting this runner.
No art, shader, quality setting, app package or shared target directory is changed.
The finite runner can execute in the foreground or an explicitly selected job.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import shutil
import statistics
import subprocess
import time

from checked_process import run_checked


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(root):
    return {
        str(p.relative_to(root)): digest(p)
        for p in sorted(root.rglob('*'))
        if p.is_file() and not {'target', 'output', '__pycache__'}.intersection(p.relative_to(root).parts)
    }


def require_tests(log):
    matches = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', log.read_text())
    assert matches and int(matches[-1][0]) > 0 and int(matches[-1][1]) == 0, 'Empty/failed test run: ' + str(log)


def verify_native_evidence(out, report):
    """Read back completed evidence; never rebuild or launch a native session."""
    required = [name + '-' + step for name in ('baseline', 'candidate')
                for step in ('build', 'test-build', 'intent', 'startup')]
    test_steps = ('tests-workstation_', 'tests-collision_bounds_checks',
                  'workstation_packet_transport_benchmark', 'material_body_collision_benchmark')
    required.extend(test_steps)
    evidence = {}

    def remember(path):
        evidence[str(path.relative_to(out))] = digest(path)
        return path

    def read(path):
        return json.loads(remember(path).read_text())

    for name in ('baseline', 'candidate'):
        for prefix, key in (('wormwood-', 'binary_sha256'), ('tests-', 'test_binary_sha256')):
            path = remember(out / (prefix + name))
            assert digest(path) == report[key][name], 'Recorded executable changed: ' + str(path)
        assert read(out / (name + '-intent.log'))['summary'] == report['intent'][name]
    for step in test_steps:
        require_tests(remember(out / (step + '.log')))
    assert report['art_and_shaders_byte_identical'] is True
    for backend in ('WebGPU', 'Vulkan', 'OpenGL'):
        saved = report['workstation'][backend]
        assert saved['identical_optical_packet'] and saved['identical_capture']
        packets, pixels = [], []
        for name in ('baseline', 'candidate'):
            label = backend + '-' + name + '-capture'
            required.append(label)
            directory = out / label
            captured = read(directory / ('bubble-piano-roll-' + backend + '.json'))
            assert captured['end_reason'] == 'frame_limit' and captured['frames'] == 4
            assert captured['entities_remaining'] == 0
            packets.append(read(directory / 'workstation-optical-frame.json'))
            pixels.append(remember(directory / ('triangle-' + backend + '.ppm')).read_bytes())
        assert packets[0] == packets[1] and pixels[0] == pixels[1], backend + ' saved capture mismatch'
        order = ('baseline', 'candidate', 'candidate', 'baseline')
        assert len(saved['runs']) == len(order) and set(saved['summary']) == set(order)
        for index, (name, recorded) in enumerate(zip(order, saved['runs'])):
            label = f'{backend}-{index}-{name}'
            required.append(label)
            directory = out / label
            assert recorded['variant'] == name and Path(recorded['report']).resolve() == directory
            data = read(directory / ('workstation-benchmark-' + backend + '.json'))
            assert data['benchmark'] == recorded['benchmark'] and data['gpu_measurement'] == recorded['gpu']
            benchmark = data['benchmark']
            assert benchmark['complete'] and benchmark['dimensions'] == [640, 480]
            assert benchmark['requested_measured_frames'] == benchmark['measured_frames'] == 32
            assert len(benchmark['samples']) == 32
            remember(directory / ('performance-gpu-' + backend + '.json'))
            remember(directory / 'performance.json')
    for step in required:
        matches = [s for s in report['steps'] if s['name'] == step]
        assert matches and all(s['passed'] for s in matches), 'Nonmining step incomplete: ' + step
        remember(out / (step + '.log'))
    return evidence


def validate_mining_report(data, sites, revisits, frames):
    assert data['replay_version'] == 2 and not data['failed']
    assert data['sites'] == sites and data['revisits'] == revisits
    assert data['frames_per_site'] == data['measured_frames_per_site'] == frames
    assert data['water_snapshots'] and data['terrain_presentation'] and data['cpu_only']
    assert len(data['runs']) == sites * (revisits + 1)
    assert data['total_frames'] == sum(r['total_frames'] for r in data['runs'])
    for visit, result in enumerate(data['runs']):
        assert result['visit'] == visit and result['site'] == visit % sites
        assert result['measured_frames'] == frames
        assert result['total_frames'] == frames + result['settling_frames']
        assert result['region_frames_after'] - result['region_frames_before'] >= result['total_frames'] - 36
        assert len(result['regions']) == min(visit + 1, sites)
        assert all(not r['physics']['halted'] for r in result['regions'])
        assert result['idle']['samples'] == 36 and result['live_step']['samples'] == frames - 37
        for metric in ('mesh_transport', 'body_collision', 'water_snapshot', 'water_index', 'terrain_presentation_cpu'):
            assert result[metric]['samples'] == frames, 'Measured/settling samples mixed: ' + metric


def resume_mining(out, smoke):
    """Retain native evidence and permit one identical test-fixture correction."""
    report_path = out / 'report.json'
    report = json.loads(report_path.read_text())
    assert not report['complete'], 'This comparison is already complete'
    native_evidence = verify_native_evidence(out, report)
    roots = {name: out / name for name in ('baseline', 'candidate')}
    current = {name: manifest(root) for name, root in roots.items()}
    correction = 'Host/MiningBenchmark.rs'
    changes = {}
    for name in roots:
        original = report['snapshots'][name]
        changed = {p for p in original.keys() | current[name].keys()
                   if original.get(p) != current[name].get(p)}
        assert changed == {correction}, 'Resume permits only the mining fixture correction: ' + str(sorted(changed))
        changes[name] = {'before': original[correction], 'after': current[name][correction]}
    assert changes['baseline'] == changes['candidate'], 'Both variants must receive exactly the same fixture correction'
    assert (roots['baseline'] / correction).read_bytes() == (roots['candidate'] / correction).read_bytes()
    # Original snapshots/hashes continue to describe the completed native and
    # regression evidence. Corrected mining test sources are recorded separately.
    serial = len(report.get('mining_resumptions', [])) + 1
    archive = out / f'report-before-mining-resume-{serial}.json'
    with archive.open('x') as destination:
        destination.write(report_path.read_text())
    attempt = {'attempt': serial, 'started_unix_seconds': time.time(), 'complete': False,
               'archived_report': str(archive), 'archived_report_sha256': digest(archive),
               'previous_failure': report.get('failure'), 'source': correction, 'digests': changes,
               'reason': 'The 90-frame fixture moved sites before the accepted dig/pour strokes finished, activating only 2 of 8 intended regions. Replay v2 finishes each stroke at its site and records extra GameStep settling separately from the unchanged measured samples.',
               'native_evidence_sha256': native_evidence, 'smoke_requested': smoke, 'runs': []}
    report.setdefault('mining_resumptions', []).append(attempt)
    report['mining_snapshots'] = current
    report.pop('failure', None)

    def save():
        temporary = out / 'report.json.tmp'
        temporary.write_text(json.dumps(report, indent=2) + '\n')
        temporary.replace(report_path)

    def run(label, argv, root, env, timeout):
        log = out / (label + '.log')
        assert not log.exists(), 'Refusing to overwrite previous evidence: ' + str(log)
        start = time.monotonic()
        print(label, flush=True)
        try:
            run_checked([str(x) for x in argv], cwd=root, log=log, timeout=timeout, env=env)
        except BaseException as error:
            report['steps'].append({'name': label, 'passed': False, 'seconds': time.monotonic() - start,
                                    'error': str(error), 'mining_resume': serial})
            save()
            if log.exists():
                print(log.read_text(errors='replace')[-6000:], flush=True)
            raise
        report['steps'].append({'name': label, 'passed': True, 'seconds': time.monotonic() - start,
                                'mining_resume': serial})
        save()
        return log

    save()
    try:
        build_env = os.environ.copy()
        runtime_env = {k: v for k, v in build_env.items() if not k.startswith(('WORMWOOD_', 'MIRROR_MINING_'))}
        tests = {}
        for name, root in roots.items():
            target = out / ('build-target' if name == 'baseline' else 'candidate-target')
            log = run(f'mining-resume-{serial}-{name}-test-build',
                      ['cargo', 'test', '--offline', '--locked', '--release', '--lib', '--no-run', '--message-format=json'],
                      root, dict(build_env, CARGO_TARGET_DIR=str(target)), 1200)
            executables = set()
            for line in log.read_text().splitlines():
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if event.get('reason') == 'compiler-artifact' and event.get('profile', {}).get('test') and event.get('executable'):
                    executables.add(event['executable'])
            assert len(executables) == 1, executables
            tests[name] = out / f'tests-{name}-mining-resume-{serial}'
            assert not tests[name].exists()
            shutil.copy2(next(iter(executables)), tests[name])
            assert digest(tests[name]) != report['test_binary_sha256'][name], 'Mining fixture was not rebuilt'
        attempt['test_binary_sha256'] = {name: digest(path) for name, path in tests.items()}
        assert len(set(attempt['test_binary_sha256'].values())) == 2, 'Variants reused the same test executable'
        save()

        def mining_run(name, label, sites, revisits):
            path = roots[name] / 'output/benchmarks/mining-20261029' / label / 'report.json'
            assert not path.parent.exists(), 'Refusing stale mining output: ' + str(path.parent)
            env = dict(runtime_env, MIRROR_MINING_LABEL=label, MIRROR_MINING_SITES=str(sites),
                       MIRROR_MINING_FRAMES='90', MIRROR_MINING_REVISITS=str(revisits),
                       MIRROR_MINING_WATER='1', MIRROR_MINING_RENDER='1')
            log = run(label, [tests[name], 'mining_stress', '--ignored', '--nocapture', '--test-threads=1'], roots[name], env, 900)
            require_tests(log)
            data = json.loads(path.read_text())
            assert data['label'] == label
            validate_mining_report(data, sites, revisits, 90)
            shutil.copy2(path, out / (label + '.json'))
            return data

        if smoke:
            for name in roots:
                label = f'mita-r{serial}-smoke-{name}'
                report.setdefault('mining_smoke', {})[label] = mining_run(name, label, 1, 0)
                save()
        for index, name in enumerate(('baseline', 'candidate', 'candidate', 'baseline')):
            label = f'mita-r{serial}-{name}-{index}'
            report['mining'][label] = mining_run(name, label, 8, 1)
            attempt['runs'].append({'variant': name, 'label': label})
            save()
        for name, root in roots.items():
            assert manifest(root) == current[name], 'Mining snapshot changed during execution: ' + name
        assert verify_native_evidence(out, report) == native_evidence, 'Completed native evidence changed during resume'
        attempt['complete'] = True
        attempt['finished_unix_seconds'] = time.time()
        report['complete'] = True
        save()
        print(json.dumps({'complete': True, 'report': str(report_path), 'mining_runs': attempt['runs'],
                          'native_evidence_reused': True}), flush=True)
    except BaseException as error:
        attempt['failure'] = str(error)
        report['failure'] = str(error)
        save()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, help='Contains baseline/ and candidate/ frozen source trees')
    parser.add_argument('--resume-mining', action='store_true', help='Reuse completed native evidence; rebuild only corrected mining tests')
    parser.add_argument('--mining-smoke', action='store_true', help='Before resumed ABBA runs, verify one site on each variant')
    args = parser.parse_args()
    out = args.directory.resolve()
    if args.mining_smoke and not args.resume_mining:
        parser.error('--mining-smoke requires --resume-mining')
    if args.resume_mining:
        return resume_mining(out, args.mining_smoke)
    roots = {name: out / name for name in ('baseline', 'candidate')}
    assert all((p / 'Cargo.toml').is_file() for p in roots.values())
    report = {'complete': False, 'steps': [], 'snapshots': {}, 'workstation': {}, 'mining': {},
              'scope': 'Local Mac release CPU and native GPU measurements; not iPhone FPS.',
              'quality': 'Identical authored assets, shaders, resolution, optical budgets and simulation rates.',
              'packaged': False, 'uploaded': False}

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, argv, root, env=None, timeout=600):
        start = time.monotonic()
        print(name, flush=True)
        try:
            run_checked([str(x) for x in argv], cwd=root, log=out / (name + '.log'),
                        timeout=timeout, env=env)
        except BaseException as error:
            report['steps'].append({'name': name, 'passed': False,
                                    'seconds': time.monotonic() - start, 'error': str(error)})
            save()
            print((out / (name + '.log')).read_text(errors='replace')[-6000:], flush=True)
            raise
        report['steps'].append({'name': name, 'passed': True, 'seconds': time.monotonic() - start})
        save()
        return out / (name + '.log')

    try:
        for name, root in roots.items():
            report['snapshots'][name] = manifest(root)
        before, after = (report['snapshots'][n] for n in roots)
        changed = sorted(p for p in before.keys() | after.keys() if before.get(p) != after.get(p))
        report['changed_files'] = changed
        assert not any(p.startswith(('Art/', 'Procedures/')) for p in changed), 'Art/shader/procedure mismatch'
        report['art_and_shaders_byte_identical'] = True
        save()

        build_env = os.environ.copy()
        runtime_env = {k: v for k, v in build_env.items() if not k.startswith('WORMWOOD_')}
        binaries, tests = {}, {}
        for name, root in roots.items():
            # Cargo can regard a copied source tree as fresh in a shared target
            # directory when source mtimes were preserved. Each variant needs
            # its own build products, including its compiled manifest path.
            target = out / ('build-target' if name == 'baseline' else 'candidate-target')
            variant_env = dict(build_env, CARGO_TARGET_DIR=str(target))
            run(name + '-build', ['cargo', 'build', '--offline', '--locked', '--release', '--bin', 'wormwood'], root, variant_env, 1200)
            binaries[name] = out / ('wormwood-' + name)
            shutil.copy2(target / 'release/wormwood', binaries[name])
            log = run(name + '-test-build', ['cargo', 'test', '--offline', '--locked', '--release',
                      '--lib', '--no-run', '--message-format=json'], root, variant_env, 1200)
            executables = []
            for line in log.read_text().splitlines():
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if event.get('reason') == 'compiler-artifact' and event.get('profile', {}).get('test') and event.get('executable'):
                    executables.append(event['executable'])
            assert len(set(executables)) == 1, executables
            tests[name] = out / ('tests-' + name)
            shutil.copy2(executables[0], tests[name])
            log = run(name + '-intent', [binaries[name], '--inspect-intent', '--json'], root,
                      dict(runtime_env, WORMWOOD_ASSET_ROOT=str(root)), 30)
            report.setdefault('intent', {})[name] = json.loads(log.read_text())['summary']
            run(name + '-startup', [binaries[name]], root,
                dict(runtime_env, WORMWOOD_ASSET_ROOT=str(root), WORMWOOD_OUTPUT_ROOT=str(out / (name + '-startup'))), 30)
        assert digest(binaries['baseline']) != digest(binaries['candidate']), 'Candidate reused baseline executable'
        assert digest(tests['baseline']) != digest(tests['candidate']), 'Candidate reused baseline test executable'
        report['binary_sha256'] = {name: digest(path) for name, path in binaries.items()}
        report['test_binary_sha256'] = {name: digest(path) for name, path in tests.items()}
        assert report['intent']['candidate']['active_structural_mismatches'] <= report['intent']['baseline']['active_structural_mismatches']

        # These filters exercise validation, original-reference equality and the
        # existing workstation lifecycle. Test compilation alone is not a pass.
        for test_filter in ('workstation_', 'collision_bounds_checks'):
            log = run('tests-' + test_filter, [tests['candidate'], test_filter, '--nocapture', '--test-threads=1'], roots['candidate'])
            require_tests(log)
        for name in ('workstation_packet_transport_benchmark', 'material_body_collision_benchmark'):
            log = run(name, [tests['candidate'], name, '--ignored', '--nocapture', '--test-threads=1'], roots['candidate'])
            require_tests(log)

        for backend in ('WebGPU', 'Vulkan', 'OpenGL'):
            frames = {}
            for name in roots:
                dest = out / (backend + '-' + name + '-capture')
                dest.mkdir()
                env = dict(runtime_env, WORMWOOD_ASSET_ROOT=str(roots[name]), WORMWOOD_OUTPUT_ROOT=str(dest),
                           WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA='0')
                # The existing bubble diagnostic fixes dt=1 for all four draws.
                # Ordinary finite sessions use wall-clock easing and are not
                # suitable for exact before/after optical packet comparison.
                run(dest.name, [binaries[name], '--check-bubble-notes', backend], roots[name], env, 120)
                captured = json.loads((dest / ('bubble-piano-roll-' + backend + '.json')).read_text())
                assert captured['end_reason'] == 'frame_limit' and captured['frames'] == 4
                assert captured['entities_remaining'] == 0
                frames[name] = json.loads((dest / 'workstation-optical-frame.json').read_text())
            assert frames['baseline'] == frames['candidate'], backend + ' optical packet changed'
            old = (out / (backend + '-baseline-capture') / ('triangle-' + backend + '.ppm')).read_bytes()
            new = (out / (backend + '-candidate-capture') / ('triangle-' + backend + '.ppm')).read_bytes()
            assert old == new, backend + ' capture pixels changed'
            report['workstation'][backend] = {'identical_optical_packet': True, 'identical_capture': True, 'runs': []}
            # ABBA order balances warm-device/order effects. The benchmark's own
            # replay fixes dt and dimensions and reports warmup separately.
            for index, name in enumerate(('baseline', 'candidate', 'candidate', 'baseline')):
                dest = out / f'{backend}-{index}-{name}'
                dest.mkdir()
                env = dict(runtime_env, WORMWOOD_ASSET_ROOT=str(roots[name]), WORMWOOD_OUTPUT_ROOT=str(dest),
                           WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA='0')
                run(dest.name, [binaries[name], '--benchmark-workstation', backend, '--frames', '32'], roots[name], env, 125)
                data = json.loads((dest / ('workstation-benchmark-' + backend + '.json')).read_text())
                assert data['benchmark']['complete'], 'Incomplete replay'
                assert data['benchmark']['dimensions'] == [640, 480], 'Unexpected viewport'
                report['workstation'][backend]['runs'].append({'variant': name, 'report': str(dest),
                                                              'benchmark': data['benchmark'], 'gpu': data['gpu_measurement']})
                save()
            summary = {}
            for name in roots:
                runs = [r for r in report['workstation'][backend]['runs'] if r['variant'] == name]
                updates = [s['update_ms'] for r in runs for s in r['benchmark']['samples'] if s['phase'] >= 2]
                summary[name] = {'moving_update_wall_ms_median': statistics.median(updates),
                                 'frame_ms_mean': statistics.mean(r['benchmark']['frame_ms_mean'] for r in runs),
                                 'gpu_complete': all(r['gpu']['complete'] for r in runs),
                                 'gpu_ms_mean': statistics.mean(r['gpu']['gpu_ms_mean'] for r in runs)
                                  if all(r['gpu']['complete'] for r in runs) else None}
            report['workstation'][backend]['summary'] = summary
            save()

        # Existing real GameStep workload includes revisit, water snapshot/index,
        # body collision, CPU terrain presentation, and full-vs-culled parity.
        for index, name in enumerate(('baseline', 'candidate', 'candidate', 'baseline')):
            label = f'mita-{name}-{index}'
            env = dict(runtime_env, MIRROR_MINING_LABEL=label, MIRROR_MINING_SITES='8',
                       MIRROR_MINING_FRAMES='90', MIRROR_MINING_REVISITS='1',
                       MIRROR_MINING_WATER='1', MIRROR_MINING_RENDER='1')
            run(label, [tests[name], 'mining_stress', '--ignored', '--nocapture', '--test-threads=1'], roots[name], env, 900)
            path = roots[name] / 'output/benchmarks/mining-20261029' / label / 'report.json'
            data = json.loads(path.read_text())
            assert not data['failed'] and len(data['runs']) == 16
            shutil.copy2(path, out / (label + '.json'))
            report['mining'][label] = data
            save()
        for name, root in roots.items():
            assert manifest(root) == report['snapshots'][name], 'Frozen snapshot changed: ' + name
        report['complete'] = True
        save()
        print(json.dumps({'complete': True, 'report': str(out / 'report.json'),
                          'workstation': {k: v['summary'] for k, v in report['workstation'].items()}}), flush=True)
    except BaseException as error:
        report['failure'] = str(error)
        save()
        raise


if __name__ == '__main__':
    main()
