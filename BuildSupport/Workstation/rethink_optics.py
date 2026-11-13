#!/usr/bin/env python3
"""Private, finite canonical-WGSL comparisons; never builds or installs the app."""
from pathlib import Path
import argparse
import array
import datetime
import json
import math
import re
import shutil
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'output/checks/workstation-performance-20261031-051718-694160'
FEATURE_PREDICATES = {'bubbles': [234, 0, '<0.5'], 'spheres': [240, 0, '<0.5'],
                      'custom_panels': [245, 0, '<0.5'], 'timeline_meshes': [248, 0, '<0.5']}
sys.path.insert(0, str(ROOT / 'BuildSupport/macOS'))
sys.path.insert(0, str(ROOT / 'BuildSupport/Workstation'))
from build_combined import clean_env, digest, ppm_info, png_info, read_json, require, run_checked, tail
from check_workstation_performance import compare_captures


def named_paths(items):
    result = {}
    for value in items:
        name, separator, path = value.partition('=')
        require(separator and re.fullmatch(r'[A-Za-z0-9_-]{1,64}', name), 'Use NAME=PATH with a simple name')
        require(name not in result, f'Duplicate input name: {name}')
        result[name] = Path(path).resolve()
    return result


def uint(value, label):
    require(isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and 0 <= value <= 16_777_216 and int(value) == value,
            f'Invalid packet integer: {label}')
    return int(value)


def finite_binary(path, expected_rows, packed=False):
    require(path.stat().st_size == expected_rows * 16, f'Binary size differs from manifest: {path}')
    data = path.read_bytes()
    values = array.array('f')
    values.frombytes(data)
    if sys.byteorder != 'little':
        values.byteswap()
    require(all(math.isfinite(v) for v in values), f'Nonfinite immutable rows: {path}')
    if packed:
        require(all(0 <= v <= 65535 and v.is_integer() for v in values), f'Invalid packed density: {path}')
    return data


def build_packet(frame_path, assets, source_texts, output):
    frame = read_json(frame_path)
    rows = frame['rows']
    require(isinstance(rows, list) and 256 <= len(rows) <= 1_000_000
            and all(isinstance(r, list) and len(r) == 4 and all(isinstance(v, (int, float))
                    and math.isfinite(v) for v in r) for r in rows), 'Invalid optical frame rows')
    frame_count, lut_edge = uint(rows[7][0], 'LUT base'), uint(rows[7][1], 'LUT edge')
    require(frame_count == len(rows) and lut_edge == 57, 'Incompatible dynamic packet/LUT ABI')
    require(rows[8][:2] == [640, 480], 'Probe requires the actual captured 640x480 sampling; no rescaling')
    require(frame.get('user_style') is None and rows[245] == [0, 0, 0, 0],
            'User-style packets require their own geometry and shader; this experiment uses built-in gems')
    directory = assets / 'Art/AudioWorkstation'
    manifests = {}
    for name, relative in {
        'monitor': 'InputMonitor/runtime.json', 'edison': 'EdisonHardware/runtime.json',
        'connected': 'EdisonHardware/connected.json', 'bubbles': 'BubbleNotes/BubbleNotes_runtime.json',
        'controls': 'ControlEngraving/control_engraving.json', 'timeline': 'Timeline/runtime.json',
        'engraving': 'LaserEngraving/engraving.json',
    }.items():
        manifests[name] = read_json(directory / relative)
    segments = []
    def append(name, base, capacity, used, relative, packed=False, expected_hash=None):
        base, capacity, used = (uint(v, name) for v in (base, capacity, used))
        require(0 < used <= capacity and base + capacity <= 8_388_608, f'Invalid immutable segment: {name}')
        path = directory / relative
        data = finite_binary(path, used, packed)
        if expected_hash:
            require(digest(path).lower() == str(expected_hash).lower(), f'Immutable hash mismatch: {name}')
        segments.append({'name': name, 'base': base, 'capacity': capacity, 'used': used,
                         'path': str(path), 'sha256': digest(path), 'data': data})
    lut_count = lut_edge ** 3
    append('AgX', frame_count, lut_count, lut_count, 'GemPanels/GemPanels_AgX.bin')
    engraving = manifests['engraving']
    density_base = frame_count + lut_count
    density_rows = uint(engraving['glyph_count'], 'glyph count') * uint(engraving['density_texels_per_glyph'], 'mip texels') // 2
    require(uint(rows[224][0], 'density pointer') == density_base, 'Frame density ABI mismatch')
    append('SSLE-density', density_base, density_rows, density_rows, 'LaserEngraving/density.bin',
           True, engraving['density_sha256'])
    for name, relative in (('monitor', 'InputMonitor/runtime.bin'), ('edison', 'EdisonHardware/runtime.bin'),
                           ('connected', 'EdisonHardware/connected.bin'),
                           ('bubbles', 'BubbleNotes/BubbleNotes_runtime.bin'), ('timeline', 'Timeline/runtime.bin')):
        m = manifests[name]
        append(name, m['base'], m['capacity'], m['rows'], relative, expected_hash=m.get('runtime_sha256'))
    controls = manifests['controls']
    controls_base = (uint(manifests['bubbles']['base'], 'bubble base') + uint(manifests['bubbles']['capacity'], 'bubble capacity') + 255) & ~255
    controls_capacity = uint(manifests['timeline']['base'], 'timeline base') - controls_base
    require(uint(controls['native_header_row'], 'control density header') == 244
            and rows[244][:2] == [controls_base, controls['density_rows']], 'Control density ABI mismatch')
    require(controls['density_binary'] == 'control_engraving.bin', 'Unexpected control density filename')
    append('control-density', controls_base, controls_capacity, controls['density_rows'],
           'ControlEngraving/control_engraving.bin', True, controls['density_sha256'])
    # Capacities and reserved alignment follow Render/Workstation.rs and the
    # frozen version's UserStyle constants, rather than historical packet offsets.
    matches = re.findall(r'pub\(crate\) const ROWS: usize = ([\d_]+);', source_texts['Workstation/UserStyle.rs'])
    require(len(matches) == 1, 'Cannot establish the frozen user-style reservation')
    style_capacity = int(matches[0].replace('_', ''))
    segments.sort(key=lambda s: s['base'])
    previous_end = frame_count
    for segment in segments:
        require(segment['base'] >= previous_end, f'Overlapping packet capacity: {segment["name"]}')
        previous_end = segment['base'] + segment['capacity']
    require(manifests['monitor']['base'] == density_base + density_rows
            and manifests['edison']['base'] == manifests['monitor']['base'] + manifests['monitor']['capacity']
            and manifests['bubbles']['base'] == (manifests['edison']['base'] + manifests['edison']['capacity'] + 255) & ~255
            and manifests['connected']['base'] == manifests['timeline']['base'] + manifests['timeline']['capacity'],
            'Immutable manifest chain disagrees with the native packet ABI')
    style_base = (previous_end + 255) & ~255
    storage_rows = ((style_base + style_capacity + 255) // 256) * 256
    require(storage_rows * 16 <= 128 * 1024 * 1024, 'Optical buffer exceeds the bounded probe size')
    data = bytearray(storage_rows * 16)
    for index, row in enumerate(rows):
        struct.pack_into('<4f', data, index * 16, *row)
    for segment in segments:
        start = segment['base'] * 16
        data[start:start + len(segment['data'])] = segment['data']
    output.write_bytes(data)
    return {'frame': str(frame_path), 'frame_sha256': digest(frame_path), 'packet': str(output),
            'packet_sha256': digest(output), 'bytes': len(data), 'dimensions': [640, 480],
            'dynamic_rows': frame_count, 'user_style': {'base': style_base, 'capacity': style_capacity, 'zeroed': True},
            'absent_features': [name for name, (row, channel, _) in FEATURE_PREDICATES.items() if rows[row][channel] < .5],
            'segments': [{k: v for k, v in s.items() if k != 'data'} for s in segments]}


def float_difference(first, second):
    arrays = []
    for path in (first, second):
        require(path.stat().st_size == 640 * 480 * 16, 'Incomplete RGBA32F readback')
        values = array.array('f')
        values.frombytes(path.read_bytes())
        if sys.byteorder != 'little':
            values.byteswap()
        require(all(math.isfinite(v) for v in values), 'Nonfinite RGBA32F comparison')
        arrays.append(values)
    total, maximum, changed, hot = 0., 0., 0, 0
    for index in range(0, len(arrays[0]), 4):
        delta = [abs(arrays[0][index+c] - arrays[1][index+c]) for c in range(3)]
        total += sum(delta)
        peak = max(delta)
        maximum = max(maximum, peak)
        changed += peak != 0
        hot += peak > 1 / 255
    return {'rgb_mean_absolute_error': total / (640 * 480 * 3), 'rgb_max_absolute_error': maximum,
            'changed_pixels': changed, 'hot_pixels_above_one_byte': hot,
            'exact_rgb_float_match': changed == 0}


def remove_fragment_entry(source):
    """The private compute experiment may use workgroup-only state."""
    match = re.search(r'@fragment\s+fn\s+fs_main\s*\([^{}]*\)[^{}]*\{', source)
    if match is None and '@fragment' not in source:
        return source
    require(match is not None, 'Cannot isolate the canonical fragment entry')
    depth, end = 1, match.end()
    while depth and end < len(source):
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    require(depth == 0, 'Unclosed canonical fragment entry')
    return source[:match.start()] + '// Fragment entry omitted for private workgroup compute probe.\n' + source[end:]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--variants-dir', type=Path, required=True, help='Directory of canonical WGSL variants; never commands or executable metadata')
    parser.add_argument('--frame', action='append', default=[], metavar='NAME=JSON', help='Additional saved optical frame with the same packet ABI')
    parser.add_argument('--native-image', action='append', default=[], metavar='NAME=PPM', help='Optional matching native screenshot for an additional frame')
    args = parser.parse_args()
    additional, native_images = named_paths(args.frame), named_paths(args.native_image)
    require('controls' not in additional, 'The frozen controls frame cannot be replaced')
    require(set(native_images) <= set(additional), 'Native image has no matching additional frame')
    frame_paths = {'controls': SOURCE / 'controls-WebGPU-candidate/workstation-optical-frame.json', **additional}
    native_images['controls'] = SOURCE / 'controls-WebGPU-candidate/triangle-WebGPU.ppm'
    variants = sorted(args.variants_dir.resolve().glob('*.wgsl'))
    require(1 <= len(variants) <= 8 and len(frame_paths) <= 4, 'Use 1..8 variants and at most four saved frames')
    for path in variants:
        require(path.is_file() and re.fullmatch(r'[A-Za-z0-9_-]{1,64}', path.stem)
                and path.stem not in ('baseline', 'baseline-repeat'), 'Invalid or reserved variant name')
    out = ROOT / 'output/checks' / ('optics-rethink-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True, exist_ok=False)
    frozen = out / 'inputs'
    frozen.mkdir()
    report = {'status': 'running', 'output': str(out), 'checks': [], 'failures': [], 'inputs': {},
              'frames': {}, 'probes': {}, 'comparisons': {}, 'packaged': False, 'promoted': False,
              'scope': 'Static canonical shading at captured 640x480, six measured compute passes after one warmup. '
                       'These captures do not establish motion fidelity, native fragment performance or sustained application FPS.',
              'hardware_exclusive': False, 'visual_review': 'pending',
              'quality': 'All captured optical rows and immutable geometry/material segments are preserved without rescaling.'}
    env = clean_env()
    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    def check(name, action):
        record = {'name': name, 'status': 'running'}
        report['checks'].append(record)
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
    def run(name, argv, timeout, verify=None):
        log, command = out / (name + '.log'), [str(v) for v in argv]
        def action():
            print(name, flush=True)
            try:
                run_checked(command, cwd=ROOT, log=log, timeout=timeout, env=env, heartbeat=30)
                return {'argv': command, 'log': str(log), 'deadline_seconds': timeout,
                        'verification': verify() if verify else None}
            except Exception:
                print(tail(log), flush=True)
                raise
        return check(name, action)
    def freeze(source, relative, maximum=128 * 1024 * 1024):
        require(source.is_file() and source.stat().st_size <= maximum, f'Missing or oversized input: {source}')
        destination = frozen / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        require(digest(source) == digest(destination), f'Input changed while freezing: {source}')
        report['inputs'][str(relative)] = {'source': str(source), 'sha256': digest(destination)}
        return destination
    def preview(prefix):
        return run(prefix.name + '-png', ['/usr/bin/sips', '-s', 'format', 'png',
                   prefix.with_suffix('.ppm'), '--out', prefix.with_suffix('.png')], 30,
                   lambda: png_info(prefix.with_suffix('.png')))
    try:
        assets = SOURCE / 'candidate/MirrorPrismals.app/Contents/Resources/EngineAssets'
        expected = read_json(SOURCE / 'candidate-before.json')
        relatives = ['Procedures/Shaders/workstation.wgsl',
            'Art/AudioWorkstation/GemPanels/GemPanels_AgX.bin',
            'Art/AudioWorkstation/LaserEngraving/engraving.json', 'Art/AudioWorkstation/LaserEngraving/density.bin',
            'Art/AudioWorkstation/InputMonitor/runtime.json', 'Art/AudioWorkstation/InputMonitor/runtime.bin',
            'Art/AudioWorkstation/EdisonHardware/runtime.json', 'Art/AudioWorkstation/EdisonHardware/runtime.bin',
            'Art/AudioWorkstation/EdisonHardware/connected.json', 'Art/AudioWorkstation/EdisonHardware/connected.bin',
            'Art/AudioWorkstation/BubbleNotes/BubbleNotes_runtime.json', 'Art/AudioWorkstation/BubbleNotes/BubbleNotes_runtime.bin',
            'Art/AudioWorkstation/ControlEngraving/control_engraving.json', 'Art/AudioWorkstation/ControlEngraving/control_engraving.bin',
            'Art/AudioWorkstation/Timeline/runtime.json', 'Art/AudioWorkstation/Timeline/runtime.bin']
        for relative in relatives:
            require(digest(assets / relative) == expected.get('Contents/Resources/EngineAssets/' + relative),
                    f'Staged candidate input changed: {relative}')
            freeze(assets / relative, Path('assets') / relative)
        source_hashes = read_json(SOURCE / 'source-before.json')
        texts = {}
        for relative in ('Render/Workstation.rs', 'Workstation/UserStyle.rs'):
            require(digest(ROOT / relative) == source_hashes.get(relative), 'Native layout source changed: ' + relative)
            texts[relative] = freeze(ROOT / relative, Path('layout') / relative).read_text()
        probe_source = freeze(ROOT / 'BuildSupport/Workstation/compute_probe.cpp', Path('compute_probe.cpp'))
        shaders = {'baseline': frozen / 'assets/Procedures/Shaders/workstation.wgsl'}
        workgroups = {'baseline': 8, 'baseline-repeat': 8}
        requirements = {'baseline': [], 'baseline-repeat': []}
        for path in variants:
            shader = freeze(path, Path('variants') / path.name, 4 * 1024 * 1024)
            text = shader.read_text()
            require('fn workstationShade(' in text and 'fn workstation_compute_main' not in text,
                    'Variants must be canonical WGSL, without a separate compute entry')
            metadata = path.with_suffix('.json')
            workgroup = 8
            absent = []
            if metadata.exists():
                settings = read_json(freeze(metadata, Path('variants') / metadata.name, 16 * 1024))
                require(isinstance(settings, dict) and 'workgroup_size' in settings
                        and type(settings['workgroup_size']) is int and settings['workgroup_size'] in (4, 8),
                        'Variant metadata requires workgroup_size: 4 or 8')
                workgroup = settings['workgroup_size']
                absent = settings.get('required_absent_features', [])
                predicates = settings.get('frame_predicates', {})
                require(isinstance(absent, list) and all(isinstance(name, str) and name in FEATURE_PREDICATES for name in absent)
                        and len(set(absent)) == len(absent) and isinstance(predicates, dict)
                        and all(name in FEATURE_PREDICATES and value == FEATURE_PREDICATES[name]
                                for name, value in predicates.items())
                        and all(name in predicates for name in absent), 'Unrecognized specialization predicates')
                report.setdefault('variant_metadata', {})[path.stem] = settings
            if re.search(r'var\s*<\s*workgroup\s*>', text):
                text = remove_fragment_entry(text)
                shader = frozen / 'compute-variants' / path.name
                shader.parent.mkdir(exist_ok=True)
                shader.write_text(text)
                report['inputs'][str(shader.relative_to(frozen))] = {'source': str(path),
                    'transformation': 'Remove canonical fragment entry for compute-only workgroup state',
                    'sha256': digest(shader)}
            shaders[path.stem] = shader
            workgroups[path.stem] = workgroup
            requirements[path.stem] = absent
        report['workgroup_sizes'] = workgroups
        for name, path in frame_paths.items():
            frame = freeze(path, Path('frames') / (name + '.json'), 32 * 1024 * 1024)
            report['frames'][name] = build_packet(frame, frozen / 'assets', texts, frozen / (name + '.bin'))
            if name in native_images:
                image = freeze(native_images[name], Path('frames') / (name + '-native.ppm'))
                require((ppm_info(image)['width'], ppm_info(image)['height']) == (640, 480), 'Native image dimensions differ')
        save()
        probe = out / 'compute-probe'
        require(run('compile-compute-probe', ['xcrun', 'clang++', '-std=c++17', '-O2', '-I/opt/homebrew/include',
                    probe_source, '-L/opt/homebrew/lib', '-lwgpu_native', '-Wl,-rpath,/opt/homebrew/lib', '-o', probe], 90),
                'Canonical compute probe did not compile')
        report['probe_sha256'] = digest(probe)
        # Repeat the baseline after every supplied variant and fixture has run.
        for variant, shader in [*shaders.items(), ('baseline-repeat', shaders['baseline'])]:
            for name, frame in report['frames'].items():
                label = name + '-' + variant
                present = sorted(set(requirements[variant]) - set(frame['absent_features']))
                if present:
                    report['checks'].append({'name': label, 'status': 'skipped_incompatible_frame',
                        'reason': 'Specialized shader requires these present features to be absent', 'features': present})
                    save()
                    continue
                prefix = out / label
                def verify(n=name, v=variant, p=prefix, packet=frame, group=workgroups[variant]):
                    result = read_json(p.with_suffix('.json'))
                    require(result.get('status') == 'completed' and result.get('measured_iterations') == 6
                            and result.get('timestamp_supported') is True and result.get('nonfinite_output_channels') == 0
                            and [result.get('width'), result.get('height')] == [640, 480]
                            and result.get('workgroup') == [group, group, 1]
                            and result.get('frame_bytes') == packet['bytes'], 'Incomplete or incompatible compute result')
                    require(len(result['passes']) == 6 and all(isinstance(x.get('gpu_ms'), (int, float))
                            and math.isfinite(x['gpu_ms']) and x['gpu_ms'] > 0 for x in result['passes']),
                            'Compute GPU query coverage incomplete')
                    ppm_info(p.with_suffix('.ppm'))
                    require(p.with_suffix('.rgba32f').stat().st_size == 640 * 480 * 16, 'Incomplete float image')
                    report['probes'].setdefault(n, {})[v] = result
                    return result
                if run(label, [probe, shader, frame['packet'], 640, 480, prefix, 6, workgroups[variant]], 180, verify):
                    preview(prefix)
        for name, probes in report['probes'].items():
            def compare_frame(n=name, results=probes):
                require('baseline' in results and 'baseline-repeat' in results, 'Baseline bracketing runs incomplete')
                first, last = results['baseline']['median_gpu_ms'], results['baseline-repeat']['median_gpu_ms']
                summary = {'baseline_start_gpu_ms': first, 'baseline_end_gpu_ms': last,
                           'baseline_drift_percent': 100 * (last / first - 1), 'variants': {}}
                base = out / (n + '-baseline')
                for variant in results:
                    if variant == 'baseline':
                        continue
                    prefix = out / (n + '-' + variant)
                    difference = out / (n + '-' + variant + '-difference-x8.ppm')
                    rgb = compare_captures(base.with_suffix('.ppm'), prefix.with_suffix('.ppm'), difference)
                    floats = float_difference(base.with_suffix('.rgba32f'), prefix.with_suffix('.rgba32f'))
                    elapsed = results[variant]['median_gpu_ms']
                    summary['variants'][variant] = {'rgb8': rgb, 'float': floats,
                        'median_gpu_ms': elapsed, 'speedup_against_start': first / elapsed,
                        'speedup_against_end': last / elapsed,
                        'same_saved_frame_exact_rgb8': rgb['identical'], 'motion_verified': False}
                    preview(difference.with_suffix(''))
                if n in native_images:
                    summary['native_vs_compute'] = compare_captures(frozen / 'frames' / (n + '-native.ppm'),
                        base.with_suffix('.ppm'), out / (n + '-native-difference-x8.ppm'))
                report['comparisons'][n] = summary
                require(summary['variants']['baseline-repeat']['rgb8']['identical']
                        and summary['variants']['baseline-repeat']['float']['exact_rgb_float_match'],
                        'Repeated baseline is not deterministic; static equivalence is inconclusive')
                return summary
            check('compare-' + name, compare_frame)
    except Exception as error:
        report['failures'].append('workflow')
        report['workflow_error'] = f'{type(error).__name__}: {error}'
    finally:
        def unchanged():
            changed = [name for name, metadata in report['inputs'].items()
                       if not (frozen / name).is_file() or digest(frozen / name) != metadata['sha256']]
            for frame in report['frames'].values():
                if digest(Path(frame['packet'])) != frame['packet_sha256']:
                    changed.append(frame['packet'])
            if 'probe_sha256' in report and digest(out / 'compute-probe') != report['probe_sha256']:
                changed.append('compute-probe')
            require(not changed, f'Frozen experiment inputs changed: {changed}')
            return {'changed': []}
        check('frozen-inputs-unchanged', unchanged)
        report['status'] = 'failed_evidence_retained' if report['failures'] else 'completed_private_experiment'
        save()
    print(json.dumps({'report': str(out / 'report.json'), 'status': report['status'],
                      'failures': report['failures'], 'promoted': False}), flush=True)
    return 1 if report['failures'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
