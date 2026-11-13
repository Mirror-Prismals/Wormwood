#!/usr/bin/env python3
"""Private isolated-ruby transport experiment; never builds or installs the app.

The caller launches this finite workload through the process-job workflow.
Reference shading comes from the independently frozen canonical WGSL. Only the
training fixture is supplied to the path compiler; the same compiled artifact
is evaluated against fractional-pixel, press and lighting holdouts.

Compiler adapter: --frame TRAIN.bin --output PROGRAM_PREFIX --depth 7
Evaluator adapter: --shader CANONICAL.wgsl --frame FIXTURE.bin
                   --program PROGRAM_PREFIX.json --output CANDIDATE_PREFIX
The evaluator writes PREFIX.wgsl and PREFIX.bin. Its packet must append to the
unaltered fixture, and its shader must be identical for every held-out fixture.
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
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
REFERENCE = ROOT / 'output/checks/optics-rethink-20261031-061827-752714/inputs'
PRIOR_NATIVE_REPORT = ROOT / 'output/checks/workstation-performance-20261031-064022-673157/report.json'
WIDTH, HEIGHT = 640, 480
MAX_PACKET = 128 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def read_json(path):
    return json.loads(Path(path).read_text())


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + '.tmp')
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')
    temporary.replace(path)


def row(data, index):
    require(0 <= index < len(data) // 16, f'Optical row outside packet: {index}')
    return list(struct.unpack_from('<4f', data, index * 16))


def put(data, index, values):
    require(0 <= index < len(data) // 16 and len(values) == 4
            and all(math.isfinite(v) for v in values), f'Invalid optical row: {index}')
    struct.pack_into('<4f', data, index * 16, *values)


def integer(value, label, maximum):
    require(math.isfinite(value) and value.is_integer() and 0 <= value <= maximum,
            f'Invalid packet integer: {label}')
    return int(value)


def finite_packet(path):
    require(path.is_file() and 256 * 16 <= path.stat().st_size <= MAX_PACKET
            and path.stat().st_size % 16 == 0, f'Invalid packet size: {path}')
    data = path.read_bytes()
    values = array.array('f')
    values.frombytes(data)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(v) for v in values), f'Nonfinite optical packet: {path}')
    return data


def update_group(data, bounds):
    pose = row(data, 192)
    low = [pose[a] + bounds[0][a] * pose[3] for a in range(3)]
    high = [pose[a] + bounds[1][a] * pose[3] for a in range(3)]
    put(data, 13, [(low[a] + high[a]) * .5 for a in range(3)] + [0.])
    put(data, 14, [(high[a] - low[a]) * .5 + 1e-7 for a in range(3)] + [0.])
    return low, high


def isolated_ruby(source):
    data = bytearray(source)
    count = len(data) // 16
    require(row(data, 8) == [float(WIDTH), float(HEIGHT), 16., 64.],
            'Reference must retain 640x480, sixteen bounces and sixty-four events')
    require(row(data, 0)[2] == 2. and row(data, 9)[0] == 3.,
            'Reference must retain two samples and three lights')
    original_pose, tree = row(data, 192), row(data, 193)
    root = integer(tree[0], 'instance0 root', count - 3)
    end = integer(tree[1], 'instance0 end', count)
    require(256 <= root < end and original_pose[3] > 0., 'Instance0 has no authored geometry')
    low, high = row(data, root)[:3], row(data, root + 1)[:3]
    require(all(low[a] < high[a] for a in range(3)), 'Ruby bounds have no volume')
    material_base = integer(tree[3] if tree[3] > .5 else row(data, 12)[1], 'material base', count - 40)
    # The selected fixture is the first authored transport cap, not a generated
    # replacement. Its first material owns the original embedded rewind glyph.
    require(row(data, material_base)[3] > .0001 and row(data, 244)[1] > 0.
            and row(data, material_base + 2)[3] > 0., 'Ruby lacks authored glass/SSLE')
    plugin_base = integer(row(data, 232)[0], 'plugin pose base', count)
    lane_base = integer(row(data, 247)[0], 'lane pose base', count)
    for instance in range(1, 65):
        if instance < 10:
            address = 192 + 2 * instance
        elif instance < 13:
            address = 226 + 2 * (instance - 10)
        elif instance < 29:
            address = plugin_base + 2 * (instance - 13) if plugin_base >= 256 else None
        elif instance < 61:
            address = lane_base + 2 * (instance - 29) if lane_base >= 256 else None
        else:
            address = plugin_base + 32 + 2 * (instance - 61) if plugin_base >= 256 else None
        if address is not None:
            put(data, address, [0.] * 4)
            put(data, address + 1, [0.] * 4)
    for panel in range(4):
        values = row(data, 32 + 40 * panel)
        values[0] = 0.
        put(data, 32 + 40 * panel, values)
    values = row(data, 0)
    values[1] = 0.
    put(data, 0, values)
    values = row(data, 12)
    values[0], values[2], values[3] = 1., 0., 0.  # Drop the old multi-instance TLAS.
    put(data, 12, values)
    for index in [15, 31, *range(226, 244), *range(245, 256)]:
        put(data, index, [0.] * 4)
    values = row(data, 212)
    values[3] = 0.  # Panel labels only; control SSLE uses its own descriptor.
    put(data, 212, values)
    values = row(data, 10)
    values[1] = 0.  # The captured field describes the old four-panel layout.
    put(data, 10, values)
    put(data, 192, [-(low[0] + high[0]) * .5, -(low[1] + high[1]) * .5, -high[2], 1.])
    bounds = [low, high]
    world_low, world_high = update_group(data, bounds)
    half_height = 1.6 * max((high[1] - low[1]) * .5,
                            (high[0] - low[0]) * .5 / (WIDTH / HEIGHT))
    put(data, 1, [0., 0., 3., 0.])
    put(data, 2, [1., 0., 0., half_height * WIDTH / HEIGHT])
    put(data, 3, [0., 1., 0., half_height])
    put(data, 4, [0., 0., -1., 1.])
    values = row(data, 5)
    values[3] = world_low[2] - .02
    put(data, 5, values)
    require(all(row(data, i) == row(source, i) for i in range(16, 31)), 'Authored lights changed')
    require(row(data, 244) == row(source, 244), 'Control density header changed')
    changed = [i for i in range(count) if source[i * 16:(i + 1) * 16] != data[i * 16:(i + 1) * 16]]
    require(all(i < 256 or plugin_base <= i < plugin_base + 40
                or lane_base <= i < lane_base + 64 for i in changed), 'Fixture changed authored data')
    return bytes(data), bounds, {'source_pose': original_pose, 'pose': row(data, 192),
        'tree': tree, 'material_base': material_base, 'local_bounds': bounds,
        'world_bounds': [world_low, world_high], 'changed_rows': changed,
        'framing_half_height_m': half_height, 'metres_per_pixel': 2 * row(data, 3)[3] / HEIGHT,
        'preserved': ['authored triangles/normals/materials', 'control engraving and density',
                      'three area lights', 'studio materials', 'AgX', 'two samples', '16 bounces', '64 events'],
        'scope': 'Synthetic isolated instance0 hero; not the full workstation or application FPS.'}


def fixture_family(training, bounds):
    # None of these changes is passed to the compiler. Light changes affect
    # evaluation only; local geometry, material and engraving remain identical.
    specifications = [
        ('train', (0., 0.), 0., None),
        ('phase-x', (.375, 0.), 0., None),
        ('phase-xy', (.5, .25), 0., None),
        ('phase-negative', (-.375, -.625), 0., None),
        ('translated', (1.25, -1.75), 0., None),
        ('pressed', (0., 0.), -.0015, None),
        ('relit', (.5, -.375), 0., (.12, -.06, .04)),
    ]
    mpp = 2 * row(training, 3)[3] / HEIGHT
    result = []
    for name, phase, press, light_shift in specifications:
        data = bytearray(training)
        pose = row(data, 192)
        pose[0] += phase[0] * mpp
        pose[1] += phase[1] * mpp
        pose[2] += press
        put(data, 192, pose)
        low, high = update_group(data, bounds)
        if light_shift:
            light = row(data, 16)
            light[:3] = [light[a] + light_shift[a] for a in range(3)]
            put(data, 16, light)
        roi = [max(0, math.floor(WIDTH * .5 + low[0] / mpp) - 8),
               max(0, math.floor(HEIGHT * .5 - high[1] / mpp) - 8),
               min(WIDTH, math.ceil(WIDTH * .5 + high[0] / mpp) + 8),
               min(HEIGHT, math.ceil(HEIGHT * .5 - low[1] / mpp) + 8)]
        result.append((name, bytes(data), {'split': 'train' if name == 'train' else 'holdout',
            'pixel_phase_xy': phase, 'press_metres': press, 'light0_shift_metres': light_shift,
            'pose': row(data, 192), 'roi': roi}))
    return result


def compare_float(reference, candidate, roi, difference_path, samples=2):
    arrays = []
    for path in (reference, candidate):
        require(path.stat().st_size == WIDTH * HEIGHT * 16, f'Incomplete float output: {path}')
        values = array.array('f')
        values.frombytes(path.read_bytes())
        if sys.byteorder != 'little':
            values.byteswap()
        require(all(math.isfinite(v) for v in values), f'Nonfinite float output: {path}')
        arrays.append(values)
    stats = {name: {'pixels': 0, 'sum': 0., 'sum_squared': 0., 'max': 0., 'changed_pixels': 0,
                    'pixels_over_1_byte': 0, 'pixels_over_2_bytes': 0} for name in ('full', 'ruby_roi')}
    coverage = {name: {str(i): 0 for i in range(samples + 1)} for name in stats}
    difference = bytearray(WIDTH * HEIGHT * 3)
    for pixel in range(WIDTH * HEIGHT):
        x, y = pixel % WIDTH, pixel // WIDTH
        delta = [abs(arrays[0][pixel * 4 + c] - arrays[1][pixel * 4 + c]) for c in range(3)]
        accepted = arrays[1][pixel * 4 + 3]
        require(accepted.is_integer() and 0 <= accepted <= samples,
                'Candidate alpha must count accepted compiled samples')
        require(arrays[0][pixel * 4 + 3] == 1., 'Canonical output alpha changed')
        peak = max(delta)
        for name in ('full', 'ruby_roi'):
            if name == 'ruby_roi' and not (roi[0] <= x < roi[2] and roi[1] <= y < roi[3]):
                continue
            item = stats[name]
            coverage[name][str(int(accepted))] += 1
            item['pixels'] += 1
            item['sum'] += sum(delta)
            item['sum_squared'] += sum(d * d for d in delta)
            item['max'] = max(item['max'], peak)
            item['changed_pixels'] += peak != 0
            item['pixels_over_1_byte'] += peak > 1 / 255
            item['pixels_over_2_bytes'] += peak > 2 / 255
        for channel, value in enumerate(delta):
            difference[pixel * 3 + channel] = min(255, int(value * 8 * 255 + .5))
    for item in stats.values():
        item['rgb_mean_absolute_error'] = item.pop('sum') / (item['pixels'] * 3)
        item['rgb_rmse'] = math.sqrt(item.pop('sum_squared') / (item['pixels'] * 3))
        item['exact_rgb_float_match'] = item['changed_pixels'] == 0
        item['rgb_max_absolute_error'] = item.pop('max')
    difference_path.write_bytes(f'P6\n{WIDTH} {HEIGHT}\n255\n'.encode() + difference)
    return {'display_encoded_float32': stats, 'difference_times_eight': str(difference_path),
            'compiled_sample_coverage': {'alpha_histograms': coverage,
                'accepted_samples': {name: sum(int(k) * v for k, v in histogram.items())
                                     for name, histogram in coverage.items()},
                'denominator_scope': 'Full frame and conservative projected ruby rectangle include background samples; '
                                     'zero alpha alone does not distinguish background from fallback.'},
            'visual_review': 'pending', 'quality_acceptance': 'not automatically inferred from averages'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--reference', type=Path, default=REFERENCE)
    parser.add_argument('--compiler', type=Path, default=Path(__file__).with_name('compiled_gem_paths.py'))
    parser.add_argument('--evaluator', type=Path, default=Path(__file__).with_name('evaluate_compiled_gem.py'))
    parser.add_argument('--depth', type=int, default=5)
    parser.add_argument('--iterations', type=int, default=6)
    parser.add_argument('--compiler-timeout', type=int, default=120)
    parser.add_argument('--preprocess-seconds', type=float, default=40.)
    parser.add_argument('--probe-timeout', type=int, default=180)
    args = parser.parse_args()
    require(1 <= args.iterations <= 12 and 4 <= args.depth <= 8 and 1 <= args.compiler_timeout <= 900
            and 1 <= args.probe_timeout <= 300 and 0 < args.preprocess_seconds <= 120
            and args.compiler_timeout >= args.preprocess_seconds + 30, 'Workload bounds exceeded')
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = (args.output or ROOT / 'output/checks' / ('compiled-gem-' + stamp)).resolve()
    require(not out.exists(), 'Use a fresh output directory; preserve previous evidence')
    out.mkdir(parents=True)
    inputs = out / 'inputs'
    inputs.mkdir()
    report = {'schema': 1, 'status': 'running', 'output': str(out), 'inputs': {}, 'fixtures': {},
        'steps': [], 'comparisons': {}, 'packaged': False, 'installed': False, 'promoted': False,
        'hardware_exclusive': False, 'visual_review': 'pending',
        'scope': 'Private isolated-ruby compute experiment; discrete held-out poses and relighting, '
                 'not continuous motion verification, a native application build or application FPS.'}

    def save():
        write_json(out / 'report.json', report)

    def freeze(source, relative, maximum=MAX_PACKET):
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
        freeze(Path(__file__), Path('check_compiled_gem.py'), 1024 * 1024)
        helper = freeze(ROOT / 'BuildSupport/checked_process.py', Path('checked_process.py'), 1024 * 1024)
        spec = importlib.util.spec_from_file_location('compiled_probe_checked_process', helper)
        checked = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(checked)
        env = os.environ.copy()
        for name in ('DYLD_INSERT_LIBRARIES', 'PYTHONPATH', 'PYTHONHOME', 'WORMWOOD_PANEL_STYLE'):
            env.pop(name, None)
        env['PYTHONDONTWRITEBYTECODE'] = '1'

        def run(name, command, timeout):
            command = [str(v) for v in command]
            item = {'name': name, 'argv': command, 'deadline_seconds': timeout, 'status': 'running'}
            report['steps'].append(item)
            save()
            started = time.monotonic()
            try:
                checked.run_checked(command, cwd=ROOT, log=out / (name + '.log'),
                                    timeout=timeout, env=env, heartbeat=30)
                item['status'] = 'passed'
            except BaseException:
                item['status'] = 'failed'
                raise
            finally:
                item['seconds'] = time.monotonic() - started
                item['log'] = str(out / (name + '.log'))
                save()

        source_packet = freeze(args.reference / 'controls.bin', Path('reference-controls.bin'))
        baseline = freeze(args.reference / 'assets/Procedures/Shaders/workstation.wgsl', Path('canonical.wgsl'), 4 * 1024 * 1024)
        require('fn workstationShade(' in baseline.read_text() and 'fn workstation_compute_main' not in baseline.read_text(),
                'Reference must be canonical WGSL without a probe entry')
        probe_source = freeze(ROOT / 'BuildSupport/Workstation/compute_probe.cpp', Path('compute_probe.cpp'), 1024 * 1024)
        compiler = freeze(args.compiler, Path('tools') / args.compiler.name, 4 * 1024 * 1024)
        evaluator = freeze(args.evaluator, Path('tools') / args.evaluator.name, 4 * 1024 * 1024)
        prior = freeze(PRIOR_NATIVE_REPORT, Path('prior-native-report.json'), 8 * 1024 * 1024)
        previous = read_json(prior)
        report['separate_prior_native_failure'] = {
            'report': str(prior), 'source_report': str(PRIOR_NATIVE_REPORT), 'sha256': digest(prior),
            'status': previous.get('status'),
            'controls_and_build_checks': [{key: item.get(key) for key in ('name', 'status', 'error')}
                for item in previous.get('checks', []) if item.get('status') == 'failed'
                and ('controls' in item.get('name', '') or 'build' in item.get('name', ''))],
            'scope': 'Earlier native validation remains separate. This headless shader experiment does not '
                     'rebuild the app or certify the subsequently edited native panic fix.'}
        # The private compiler may reuse the existing packet/flat-facet reader.
        dependency = ROOT / 'BuildSupport/Workstation/analytic_controls.py'
        if dependency.name not in (compiler.name, evaluator.name):
            freeze(dependency, Path('tools') / dependency.name, 4 * 1024 * 1024)
        original = finite_packet(source_packet)
        training, bounds, fixture_info = isolated_ruby(original)
        report['isolation'] = fixture_info
        frames = {}
        for name, data, metadata in fixture_family(training, bounds):
            path = inputs / (name + '.bin')
            path.write_bytes(data)
            report['inputs'][path.name] = {'source': 'isolated canonical packet + explicit fixture transform', 'sha256': digest(path)}
            report['fixtures'][name] = {**metadata, 'frame': str(path), 'sha256': digest(path)}
            frames[name] = path
        save()
        program = out / 'program'
        run('compile-path-program', [sys.executable, compiler, '--frame', frames['train'], '--output', program,
                                    '--depth', args.depth, '--budget-seconds', args.preprocess_seconds], args.compiler_timeout)
        require(all(program.with_suffix(suffix).is_file() for suffix in ('.json', '.bin')),
                'Path compiler did not produce PROGRAM.json and PROGRAM.bin')
        program_hashes = {suffix: digest(program.with_suffix(suffix)) for suffix in ('.json', '.bin')}
        report['program'] = {'path': str(program.with_suffix('.json')), 'sha256': program_hashes,
                             'depth': args.depth, 'preprocess_seconds': args.preprocess_seconds,
                             'training_fixture': 'train', 'holdouts_used_for_compilation': False}
        probe = out / 'compute-probe'
        run('compile-compute-probe', ['xcrun', 'clang++', '-std=c++17', '-O2', '-I/opt/homebrew/include',
            probe_source, '-L/opt/homebrew/lib', '-lwgpu_native', '-Wl,-rpath,/opt/homebrew/lib', '-o', probe], 90)
        report['probe_sha256'] = digest(probe)
        candidate_shader_hash = None
        candidate_append_hash = None
        for name, frame in frames.items():
            verify_frozen()
            require(all(digest(program.with_suffix(suffix)) == value for suffix, value in program_hashes.items()),
                    'Training program changed')
            candidate = out / (name + '-candidate-input')
            run(name + '-generate-evaluator', [sys.executable, evaluator, '--shader', baseline,
                '--frame', frame, '--program', program.with_suffix('.json'), '--output', candidate], 60)
            shader, packet = candidate.with_suffix('.wgsl'), candidate.with_suffix('.bin')
            require(shader.is_file() and packet.is_file(), 'Evaluator did not produce WGSL and packet')
            candidate_data = finite_packet(packet)
            fixture_data = frame.read_bytes()
            require(candidate_data[:len(fixture_data)] == fixture_data, 'Evaluator changed the reference fixture instead of appending program data')
            append_hash = hashlib.sha256(candidate_data[len(fixture_data):]).hexdigest()
            if candidate_append_hash is None:
                candidate_append_hash = append_hash
            require(append_hash == candidate_append_hash, 'Evaluator changed compiled tables for a held-out fixture')
            actual_shader_hash = digest(shader)
            if candidate_shader_hash is None:
                candidate_shader_hash = actual_shader_hash
            require(actual_shader_hash == candidate_shader_hash, 'Evaluator specialized on held-out state; use the same shader/program for every fixture')
            report['fixtures'][name]['candidate'] = {'shader': str(shader), 'shader_sha256': actual_shader_hash,
                'packet': str(packet), 'packet_sha256': digest(packet), 'appended_bytes': len(candidate_data) - len(fixture_data)}
            measurements = {}
            for variant, selected_shader, selected_frame in (('baseline', baseline, frame), ('candidate', shader, packet)):
                prefix = out / (name + '-' + variant)
                run(prefix.name, [probe, selected_shader, selected_frame, WIDTH, HEIGHT, prefix, args.iterations, 8], args.probe_timeout)
                measurement = read_json(prefix.with_suffix('.json'))
                require(measurement.get('status') == 'completed' and measurement.get('width') == WIDTH
                        and measurement.get('height') == HEIGHT and measurement.get('nonfinite_output_channels') == 0
                        and measurement.get('measured_iterations') == args.iterations
                        and len(measurement.get('passes', [])) == args.iterations,
                        f'Incomplete or invalid probe: {prefix}')
                measurements[variant] = measurement
            comparison = compare_float(out / (name + '-baseline.rgba32f'), out / (name + '-candidate.rgba32f'),
                                       report['fixtures'][name]['roi'], out / (name + '-difference-x8.ppm'))
            baseline_ms, candidate_ms = (measurements[v].get('median_gpu_ms') for v in ('baseline', 'candidate'))
            comparison.update(baseline_gpu_ms=baseline_ms, candidate_gpu_ms=candidate_ms,
                gpu_speedup=baseline_ms / candidate_ms if baseline_ms and candidate_ms else None,
                timestamp_supported=all(m.get('timestamp_supported') for m in measurements.values()))
            report['comparisons'][name] = comparison
            for suffix in ('baseline', 'candidate', 'difference-x8'):
                prefix = out / (name + '-' + suffix)
                run(prefix.name + '-png', ['/usr/bin/sips', '-s', 'format', 'png', prefix.with_suffix('.ppm'),
                                         '--out', prefix.with_suffix('.png')], 30)
            save()
        # A final reference dispatch bounds timing drift without changing the
        # input or replacing the independent reference with candidate output.
        prefix = out / 'train-baseline-repeat'
        run(prefix.name, [probe, baseline, frames['train'], WIDTH, HEIGHT, prefix, args.iterations, 8], args.probe_timeout)
        repeated = read_json(prefix.with_suffix('.json'))
        require(repeated.get('status') == 'completed' and repeated.get('nonfinite_output_channels') == 0
                and repeated.get('width') == WIDTH and repeated.get('height') == HEIGHT
                and repeated.get('measured_iterations') == args.iterations,
                'Reference repeat failed')
        report['baseline_repeat'] = repeated
        report['baseline_repeat_float_identical'] = (
            digest(out / 'train-baseline.rgba32f') == digest(prefix.with_suffix('.rgba32f')))
        require(report['baseline_repeat_float_identical'], 'Independent reference did not repeat deterministically')
        first = report['comparisons']['train']['baseline_gpu_ms']
        last = repeated.get('median_gpu_ms')
        report['baseline_drift_percent'] = 100 * (last / first - 1) if first and last else None
        verify_frozen()
        require(digest(probe) == report['probe_sha256']
                and all(digest(program.with_suffix(suffix)) == value for suffix, value in program_hashes.items()),
                'Frozen executable or compiled program changed')
        report['status'] = 'completed_private_experiment'
        save()
        print('COMPILED_GEM_REPORT=' + str(out / 'report.json'), flush=True)
    except BaseException as error:
        report.update(status='failed', error=f'{type(error).__name__}: {error}')
        save()
        raise


if __name__ == '__main__':
    main()
