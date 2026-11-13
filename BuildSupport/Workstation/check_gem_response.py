#!/usr/bin/env python3
"""Finite private GPU optical-response experiment; no app build or installation.

Launch through the process-job workflow. An independent canonical shader is
compared with one training-only response atlas reused across all eight fixtures.
The 256-square atlas is a material response table; rendered output remains the
original 640x480, two-sample, sixteen-bounce, sixty-four-event reference.
"""
from pathlib import Path
import argparse
import array
import datetime
import hashlib
import importlib.util
import json
import math
import os
import shutil
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / 'BuildSupport/Workstation'
REFERENCE = ROOT / 'output/checks/optics-rethink-20261031-061827-752714/inputs'
PRIOR_NATIVE = ROOT / 'output/checks/workstation-performance-20261031-064022-673157/report.json'
EDGE, ATLAS_ROWS, WIDTH, HEIGHT = 256, 60, 640, 480
MAX_BYTES = 128 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    result = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify_append(packet, fixture, atlas, fixture_hash, atlas_hash):
    fixture_size, atlas_size = fixture.stat().st_size, atlas.stat().st_size
    require(fixture_size + atlas_size <= MAX_BYTES, 'Combined packet exceeds 128 MiB')
    require(packet.stat().st_size == fixture_size + atlas_size,
            'Candidate must contain exactly the unchanged fixture followed by the shared atlas')
    with packet.open('rb') as stream:
        for count, expected, label in ((fixture_size, fixture_hash, 'fixture'),
                                       (atlas_size, atlas_hash, 'atlas')):
            value = hashlib.sha256()
            remaining = count
            while remaining:
                chunk = stream.read(min(remaining, 1024 * 1024))
                require(chunk, 'Truncated candidate packet')
                value.update(chunk)
                remaining -= len(chunk)
            require(value.hexdigest() == expected, f'Candidate changed frozen {label} bytes')


def measurement(path, width, height, iterations, rows=1):
    value = json.loads(path.read_text())
    require(value.get('status') == 'completed' and value.get('width') == width
            and value.get('height') == height and value.get('rows_per_pixel') == rows
            and value.get('output_bytes') == width * height * rows * 16
            and value.get('nonfinite_output_channels') == 0
            and value.get('measured_iterations') == iterations
            and len(value.get('passes', [])) == iterations, f'Incomplete GPU evidence: {path}')
    return value


def atlas_diagnostics(path):
    # Read once into packed float storage: do not retain a second bytes copy.
    values = array.array('f')
    with path.open('rb') as stream:
        values.fromfile(stream, EDGE * EDGE * ATLAS_ROWS * 4)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(v) for v in values), 'Nonfinite atlas coefficients')
    samples = EDGE * EDGE * 2
    covered = 0
    leaf_counts = {}
    dropped_sum, dropped_max = [0.] * 3, [0.] * 3
    kept_sum, maximum_fraction = [0.] * 3, [0.] * 3
    dropped_entries = 0
    for sample in range(samples):
        base = sample * 30 * 4
        coverage, leaves = values[base + 3], values[base + 7]
        require(coverage in (0., 1.) and leaves.is_integer() and leaves >= 0.,
                'Malformed response coverage or terminal leaf count')
        covered += int(coverage)
        key = str(int(leaves))
        leaf_counts[key] = leaf_counts.get(key, 0) + 1
        dropped = values[base + 4:base + 7]
        require(all(v >= 0. for v in dropped), 'Negative dropped transport weight')
        dropped_entries += any(v > 0. for v in dropped)
        for channel in range(3):
            kept = sum(values[base + (4 + lobe * 3) * 4 + channel] for lobe in range(8))
            require(kept >= 0., 'Negative retained transport weight')
            dropped_sum[channel] += dropped[channel]
            dropped_max[channel] = max(dropped_max[channel], dropped[channel])
            kept_sum[channel] += kept
            total = dropped[channel] + kept
            if total > 0.:
                maximum_fraction[channel] = max(maximum_fraction[channel], dropped[channel] / total)
    return {'entries': samples, 'primary_hit_entries': covered,
        'primary_hit_fraction': covered / samples, 'terminal_leaf_count_histogram': leaf_counts,
        'maximum_terminal_leaves': max(int(v) for v in leaf_counts),
        'entries_with_discarded_lobes': dropped_entries,
        'discarded_weight_rgb_sum': dropped_sum, 'discarded_weight_rgb_max': dropped_max,
        'discarded_weight_rgb_mean': [v / samples for v in dropped_sum],
        'retained_weight_rgb_sum': kept_sum,
        'maximum_discarded_fraction_rgb': maximum_fraction,
        'meaning': 'Header 0.w measures actual primary glass hits; screen alpha only measures atlas-rectangle lookup. '
                   'Header 1.rgb is nonnegative transport weight omitted by the top-eight terminal-lobe limit. '
                   'Multiplying it by a componentwise upper bound on live exterior radiance bounds omitted linear radiance; '
                   'it does not bound interpolation, engraving relighting, or display-space error.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--reference', type=Path, default=REFERENCE)
    parser.add_argument('--generator', type=Path, default=TOOLS / 'gem_response.py')
    parser.add_argument('--iterations', type=int, default=6)
    parser.add_argument('--bake-timeout', type=int, default=180)
    parser.add_argument('--probe-timeout', type=int, default=180)
    args = parser.parse_args()
    require(1 <= args.iterations <= 12 and 1 <= args.bake_timeout <= 300
            and 1 <= args.probe_timeout <= 300, 'Workload bounds exceeded')
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = (args.output or ROOT / 'output/checks' / ('gem-response-' + stamp)).resolve()
    require(not out.exists(), 'Use a fresh output directory; preserve earlier evidence')
    out.mkdir(parents=True)
    inputs = out / 'inputs'
    inputs.mkdir()
    report = {'schema': 1, 'status': 'running', 'output': str(out), 'inputs': {},
        'fixtures': {}, 'steps': [], 'comparisons': {}, 'packaged': False,
        'installed': False, 'promoted': False, 'visual_review': 'pending',
        'hardware_exclusive': False,
        'scope': 'Private isolated-ruby GPU response table. One training fixture; seven held-out poses/lighting cases. '
                 'Discrete captures do not establish continuous motion fidelity or whole-application FPS.',
        'representation': {'atlas_edge': EDGE, 'rows_per_texel': ATLAS_ROWS,
            'samples': 2, 'terminal_lobes_per_sample': 8, 'engraving_light_basis_vectors_per_sample': 4,
            'glyph_shape': 'fixed in this prototype',
            'lighting': 'Live light colors combine linearly; moved-light engraving uses the documented geometric gain approximation.',
            'quality_acceptance': 'Requires held-out image review; not inferred solely from average error or speed.'},
        'workload': {'shade_processes': 17, 'measured_shade_iterations_each': args.iterations,
            'unmeasured_warmups_each': 1, 'atlas_bake_processes': 1,
            'bake_timeout_seconds': args.bake_timeout, 'shade_timeout_seconds': args.probe_timeout,
            'generator_timeout_seconds': 60, 'compiler_timeout_seconds': 90}}

    def save():
        temporary = out / 'report.json.tmp'
        temporary.write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')
        temporary.replace(out / 'report.json')

    def freeze(source, relative, maximum=MAX_BYTES):
        source = source.resolve()
        require(source.is_file() and source.stat().st_size <= maximum, f'Missing/oversized input: {source}')
        before = digest(source)
        destination = inputs / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        require(before == digest(source) == digest(destination), f'Input changed while freezing: {source}')
        report['inputs'][str(relative)] = {'source': str(source), 'sha256': before}
        return destination

    def verify_frozen():
        for relative, item in report['inputs'].items():
            require(digest(inputs / relative) == item['sha256'], f'Frozen input changed: {relative}')

    try:
        freeze(Path(__file__), Path('tools/check_gem_response.py'), 1024 * 1024)
        fixture_helper = freeze(TOOLS / 'check_compiled_gem.py', Path('tools/check_compiled_gem.py'), 1024 * 1024)
        process_helper = freeze(ROOT / 'BuildSupport/checked_process.py', Path('tools/checked_process.py'), 1024 * 1024)
        helpers = load_module('gem_response_fixtures', fixture_helper)
        checked = load_module('gem_response_checked_process', process_helper)
        generator = freeze(args.generator, Path('tools') / args.generator.name, 4 * 1024 * 1024)
        freeze(args.generator.with_name('response_floor.py'), Path('tools/response_floor.py'), 4 * 1024 * 1024)
        probe_source = freeze(TOOLS / 'response_probe.cpp', Path('response_probe.cpp'), 1024 * 1024)
        source_packet = freeze(args.reference / 'controls.bin', Path('reference-controls.bin'))
        canonical = freeze(args.reference / 'assets/Procedures/Shaders/workstation.wgsl', Path('canonical.wgsl'), 4 * 1024 * 1024)
        require('fn workstationShade(' in canonical.read_text()
                and 'fn workstation_compute_main' not in canonical.read_text(), 'Reference is not the canonical shader')
        prior_path = freeze(PRIOR_NATIVE, Path('prior-native-report.json'), 8 * 1024 * 1024)
        previous = helpers.read_json(prior_path)
        report['separate_prior_native_failure'] = {'source': str(PRIOR_NATIVE), 'sha256': digest(prior_path),
            'status': previous.get('status'),
            'controls_and_build_checks': [{key: item.get(key) for key in ('name', 'status', 'error')}
                for item in previous.get('checks', []) if item.get('status') == 'failed'
                and ('controls' in item.get('name', '') or 'build' in item.get('name', ''))],
            'scope': 'Historical native failures; this private shader probe does not rebuild or validate the native application fix.'}
        env = os.environ.copy()
        for name in ('DYLD_INSERT_LIBRARIES', 'PYTHONPATH', 'PYTHONHOME', 'WORMWOOD_PANEL_STYLE'):
            env.pop(name, None)
        env['PYTHONDONTWRITEBYTECODE'] = '1'

        def run(name, command, timeout):
            command = [str(v) for v in command]
            step = {'name': name, 'argv': command, 'deadline_seconds': timeout,
                    'log': str(out / (name + '.log')), 'status': 'running'}
            report['steps'].append(step)
            save()
            started = time.monotonic()
            try:
                checked.run_checked(command, cwd=ROOT, log=step['log'], timeout=timeout, env=env, heartbeat=30)
                step['status'] = 'passed'
            except BaseException:
                step['status'] = 'failed'
                raise
            finally:
                step['seconds'] = time.monotonic() - started
                save()

        training, bounds, isolation = helpers.isolated_ruby(helpers.finite_packet(source_packet))
        report['isolation'] = isolation
        frames = {}
        fixtures = helpers.fixture_family(training, bounds)
        recolored = bytearray(training)
        color = helpers.row(recolored, 20)
        factors = (1.2, .6, 1.4)
        color[:3] = [color[channel] * factors[channel] for channel in range(3)]
        helpers.put(recolored, 20, color)
        fixtures.append(('recolored', bytes(recolored), {**fixtures[0][2], 'split': 'holdout',
            'light0_color_rgb_multiplier': factors, 'light0_color_rgb': color[:3],
            'description': 'Training geometry and pose; only light 0 RGB changes.'}))
        del recolored
        for name, data, metadata in fixtures:
            path = inputs / (name + '.bin')
            path.write_bytes(data)
            value = digest(path)
            report['inputs'][path.name] = {'source': 'isolated canonical packet + explicit fixture transform', 'sha256': value}
            report['fixtures'][name] = {**metadata, 'frame': str(path), 'sha256': value}
            frames[name] = path
        del training, fixtures, data
        probe = out / 'response-probe'
        run('compile-response-probe', ['xcrun', 'clang++', '-std=c++17', '-O2', '-I/opt/homebrew/include',
            probe_source, '-L/opt/homebrew/lib', '-lwgpu_native', '-Wl,-rpath,/opt/homebrew/lib', '-o', probe], 90)
        report['probe_sha256'] = digest(probe)
        bake = out / 'bake'
        run('prepare-bake', [sys.executable, generator, 'prepare', '--shader', canonical,
                            '--frame', frames['train'], '--output', bake, '--edge', EDGE], 60)
        require(all(bake.with_suffix(s).is_file() for s in ('.json', '.wgsl', '.bin')),
                'Prepare did not produce bake metadata, shader and packet')
        metadata = helpers.read_json(bake.with_suffix('.json'))
        require(metadata.get('rows_per_pixel') == ATLAS_ROWS and metadata.get('edge') == EDGE,
                'Unexpected response atlas schema')
        bake_data = helpers.finite_packet(bake.with_suffix('.bin'))
        require(helpers.row(bake_data, 8) == [float(EDGE), float(EDGE), 16., 64.]
                and helpers.row(bake_data, 0)[2] == 2., 'Bake changed required optical budgets or viewport')
        del bake_data
        bake_hashes = {suffix: digest(bake.with_suffix(suffix)) for suffix in ('.json', '.wgsl', '.bin')}
        atlas = out / 'atlas'
        run('bake-atlas', [probe, bake.with_suffix('.wgsl'), bake.with_suffix('.bin'), EDGE, EDGE,
                          atlas, 1, 8, ATLAS_ROWS], args.bake_timeout)
        bake_measurement = measurement(atlas.with_suffix('.json'), EDGE, EDGE, 1, ATLAS_ROWS)
        atlas_path = atlas.with_suffix('.rgba32f')
        require(atlas_path.stat().st_size == EDGE * EDGE * ATLAS_ROWS * 16, 'Incomplete response atlas')
        diagnostics = atlas_diagnostics(atlas_path)
        atlas_hash = digest(atlas_path)
        require(frames['train'].stat().st_size + atlas_path.stat().st_size <= MAX_BYTES,
                'Response atlas plus fixture exceeds 128 MiB')
        report['atlas'] = {'path': str(atlas_path), 'sha256': atlas_hash, 'bytes': atlas_path.stat().st_size,
            'metadata': str(bake.with_suffix('.json')), 'prepared_input_hashes': bake_hashes,
            'training_fixture': 'train', 'holdouts_used_for_baking': False,
            'combined_packet_bytes': frames['train'].stat().st_size + atlas_path.stat().st_size,
            'gpu_measurement': bake_measurement, 'diagnostics': diagnostics}
        save()

        def verify_atlas():
            require(digest(atlas_path) == atlas_hash, 'Shared response atlas changed')
            require(all(digest(bake.with_suffix(suffix)) == value for suffix, value in bake_hashes.items()),
                    'Baker metadata or prepared input changed')

        shader_hash = None
        for name, frame in frames.items():
            verify_frozen()
            verify_atlas()
            candidate = out / (name + '-response-input')
            run(name + '-evaluate', [sys.executable, generator, 'evaluate', '--shader', canonical,
                '--frame', frame, '--atlas', atlas_path, '--metadata', bake.with_suffix('.json'), '--output', candidate], 60)
            shader, packet = candidate.with_suffix('.wgsl'), candidate.with_suffix('.bin')
            require(shader.is_file() and packet.is_file() and candidate.with_suffix('.json').is_file(),
                    'Evaluator did not produce shader, packet and report')
            verify_append(packet, frame, atlas_path, report['fixtures'][name]['sha256'], atlas_hash)
            current_shader_hash = digest(shader)
            if shader_hash is None:
                shader_hash = current_shader_hash
            require(current_shader_hash == shader_hash, 'Evaluator specialized shader on a held-out fixture')
            report['fixtures'][name]['response'] = {'shader': str(shader), 'shader_sha256': shader_hash,
                'packet': str(packet), 'packet_sha256': digest(packet),
                'evaluator_report': helpers.read_json(candidate.with_suffix('.json'))}
            timings = {}
            for variant, selected_shader, selected_frame in (('baseline', canonical, frame), ('response', shader, packet)):
                prefix = out / (name + '-' + variant)
                run(prefix.name, [probe, selected_shader, selected_frame, WIDTH, HEIGHT, prefix, args.iterations, 8, 1], args.probe_timeout)
                timings[variant] = measurement(prefix.with_suffix('.json'), WIDTH, HEIGHT, args.iterations)
            comparison = helpers.compare_float(out / (name + '-baseline.rgba32f'), out / (name + '-response.rgba32f'),
                report['fixtures'][name]['roi'], out / (name + '-difference-x8.ppm'))
            comparison['response_sample_coverage'] = comparison.pop('compiled_sample_coverage')
            comparison['response_sample_coverage']['denominator_scope'] = (
                'Alpha counts samples inside the response rectangle, including background texels. '
                'This is lookup coverage, not geometric hit coverage or an accuracy certificate; '
                'actual primary-hit coverage is reported separately in atlas diagnostics.')
            first, second = (timings[variant].get('median_gpu_ms') for variant in ('baseline', 'response'))
            comparison.update(baseline_gpu_ms=first, response_gpu_ms=second,
                gpu_speedup=first / second if first and second else None,
                timestamp_supported=all(t.get('timestamp_supported') for t in timings.values()))
            report['comparisons'][name] = comparison
            for variant in ('baseline', 'response', 'difference-x8'):
                prefix = out / (name + '-' + variant)
                run(prefix.name + '-png', ['/usr/bin/sips', '-s', 'format', 'png', prefix.with_suffix('.ppm'),
                    '--out', prefix.with_suffix('.png')], 30)
            save()
        prefix = out / 'train-baseline-repeat'
        run(prefix.name, [probe, canonical, frames['train'], WIDTH, HEIGHT, prefix, args.iterations, 8, 1], args.probe_timeout)
        repeated = measurement(prefix.with_suffix('.json'), WIDTH, HEIGHT, args.iterations)
        report['baseline_repeat'] = repeated
        report['baseline_repeat_float_identical'] = (
            digest(out / 'train-baseline.rgba32f') == digest(prefix.with_suffix('.rgba32f')))
        require(report['baseline_repeat_float_identical'], 'Independent reference did not repeat deterministically')
        first, last = report['comparisons']['train']['baseline_gpu_ms'], repeated.get('median_gpu_ms')
        report['baseline_drift_percent'] = 100 * (last / first - 1) if first and last else None
        verify_frozen()
        verify_atlas()
        require(digest(probe) == report['probe_sha256'], 'Frozen GPU probe changed')
        report['status'] = 'completed_private_experiment'
        save()
        print('GEM_RESPONSE_REPORT=' + str(out / 'report.json'), flush=True)
    except BaseException as error:
        report.update(status='failed', error=f'{type(error).__name__}: {error}')
        save()
        raise


if __name__ == '__main__':
    main()
