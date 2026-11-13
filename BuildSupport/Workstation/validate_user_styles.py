#!/usr/bin/env python3
"""Finite user-style checks, native captures, and evidence for visual review.

Launch this workload through Codex Process Jobs. Process success and a nonempty
image establish capture evidence, not a visual-quality verdict. The report
deliberately leaves visual review and live interaction pending.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import statistics
import struct
import subprocess
import sys
import time
import zlib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_hashes():
    files = set()
    for directory in ('Workstation', 'BaseSystem', 'Host', 'Systems', 'Platform', 'Render',
                      'Procedures', 'BuildSupport/Workstation', 'Art/AudioWorkstation/StyleExamples'):
        files.update(path for path in (ROOT / directory).rglob('*') if path.is_file()
                     and path.suffix in ('.rs', '.json', '.glsl', '.wgsl', '.frag',
                                         '.vert', '.py', '.mm', '.cpp', '.h', '.txt', '.glb'))
    files.update(ROOT / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'lib.rs', 'main.rs'))
    return {str(path.relative_to(ROOT)): sha256(path) for path in sorted(files) if path.is_file()}


def runtime_files():
    patterns = ['Project.json', 'Systems/*.json', 'BaseSystem/registry.json',
                'BaseSystem/circuit_breaker.json', 'Procedures/**/*.json',
                'Procedures/Assets/**/*.bin', 'Procedures/Assets/**/*.rgba',
                'Procedures/Assets/**/*.glb',
                'Procedures/Shaders/workstation*', 'Art/AudioWorkstation/**/*.glb',
                'Art/AudioWorkstation/**/*.bin', 'Art/AudioWorkstation/**/*.json',
                'Art/AudioWorkstation/**/*.wgsl']
    return sorted({p for pattern in patterns for p in ROOT.glob(pattern) if p.is_file()})


def copy_runtime(destination):
    hashes = {}
    for path in runtime_files():
        relative = path.relative_to(ROOT)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        hashes[str(relative)] = sha256(target)
    return hashes


def resume_runtime(evidence, destination, binary):
    """Preserve an exact older binary/snapshot; repair only proven omissions."""
    previous = json.loads((evidence / 'report.json').read_text())
    source_record = evidence / 'validated-source-before.json'
    if not source_record.is_file():
        source_record = evidence / 'source-before.json'
    recorded_source = json.loads(source_record.read_text())
    previous_binary = evidence / 'wormwood-style-validation'
    if sha256(previous_binary) != previous.get('binary_sha256'):
        raise ValueError('Previous executable no longer matches its evidence hash')
    recorded_assets = {**previous.get('asset_sha256', {}),
                       **previous.get('validation_variant_sha256', {})}
    previous_assets = evidence / 'assets'
    actual = {str(path.relative_to(previous_assets)): path
              for path in previous_assets.rglob('*') if path.is_file()}
    if actual.keys() != recorded_assets.keys():
        raise ValueError('Previous asset snapshot has missing or unrecorded files')
    for relative, path in actual.items():
        if sha256(path) != recorded_assets[relative]:
            raise ValueError('Previous asset snapshot changed: ' + relative)
    source_steps = previous.get('source_check_steps', previous.get('steps', []))
    required = {'test-user_style', 'test-style_shader', 'test-browser_',
                'test-workstation_ssle_', 'desktop-build', 'intent'}
    passed = {step['name'] for step in source_steps if step.get('status') == 'passed'}
    if required - passed:
        raise ValueError('Previous evidence lacks passing source checks: ' + ', '.join(sorted(required - passed)))
    # Do not pair this executable with newer Systems, startup order or shaders.
    # The original source fingerprint proves the version of a missing asset;
    # it is not necessary for today's unrelated source files to match it.
    shutil.copytree(previous_assets, destination)
    shutil.copy2(previous_binary, binary)
    if sha256(binary) != previous['binary_sha256']:
        raise ValueError('Executable changed while copying the previous snapshot')
    for relative, expected in recorded_assets.items():
        if sha256(destination / relative) != expected:
            raise ValueError('Asset changed while copying the previous snapshot: ' + relative)
    additions = {}
    skipped = []
    changed = []
    for path in runtime_files():
        relative = str(path.relative_to(ROOT))
        target = destination / relative
        if target.exists() or not relative.startswith('Procedures/'):
            continue
        expected = recorded_source.get(relative)
        if expected is None:
            skipped.append(relative)
            continue
        if sha256(path) != expected:
            changed.append(relative)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)
        if sha256(target) != expected:
            raise ValueError('Missing dependency changed while copying: ' + relative)
        additions[relative] = expected
    shutil.copy2(source_record, binary.parent / 'validated-source-before.json')
    return {**recorded_assets, **additions}, {
        'resumed_from': str(evidence),
        'source_evidence_origin': previous.get('source_evidence_origin', str(evidence)),
        'source_check_steps': [step for step in source_steps if step.get('name') in required],
        'source_check_scope': 'inherited passing checks of the frozen executable; current workspace was not rebuilt',
        'intent_summary': previous.get('intent_summary'),
        'intent_scope': 'inherited structural evidence for frozen metadata; behavior and visual review remain separate',
        'snapshot_repair_sha256': additions,
        'unrecorded_dependencies_not_added': skipped,
        'changed_dependencies_not_added': changed,
        'inherited_source_changes_during_build': previous.get('inherited_source_changes_during_build',
                                                             previous.get('sources_changed_during_run', [])),
        'historical_fingerprint_limit': 'Older evidence may omit Render sources; frozen executable and asset hashes are retained.'}


def ppm_to_png(source, target):
    data = source.read_bytes()
    match = re.match(rb'P6\s+(\d+)\s+(\d+)\s+255[\r]?\n', data)
    if not match:
        raise ValueError(f'Unsupported or missing P6 capture header: {source}')
    width, height = map(int, match.groups())
    pixels = data[match.end():]
    if len(pixels) != width * height * 3 or not pixels or max(pixels) == min(pixels):
        raise ValueError(f'Empty, constant, or truncated native capture: {source}')

    def chunk(kind, contents):
        return (struct.pack('>I', len(contents)) + kind + contents
                + struct.pack('>I', zlib.crc32(kind + contents) & 0xffffffff))

    rows = b''.join(b'\x00' + pixels[i:i + width * 3] for i in range(0, len(pixels), width * 3))
    target.write_bytes(b'\x89PNG\r\n\x1a\n'
        + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
        + chunk(b'IDAT', zlib.compress(rows, 6)) + chunk(b'IEND', b''))
    return {'width': width, 'height': height, 'ppm_sha256': sha256(source),
            'png': str(target), 'visual_review': 'pending'}


def native_summary(log):
    for line in reversed(log.read_text(errors='replace').splitlines()):
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and value.get('mode') == 'workstation' and 'end_reason' in value:
            return value
    raise ValueError('Native log is missing the final workstation result: ' + str(log))


def wood_shadow_fixture(frame, image):
    """Sample actual rendered pixels on the known lit wood fixture, not user art.

    The old OpenGL capture produced skinny/broad ratios 0.024 and 0.025 at
    these top/bottom samples. A 0.30 threshold leaves room for procedural
    grain while detecting the broad almost-black self-shadow wedge.
    """
    raw = image.read_bytes()
    header = re.match(rb'P6\s+(\d+)\s+(\d+)\s+255[\r]?\n', raw)
    if not header:
        raise ValueError('Missing wood fixture P6 capture')
    width, height = map(int, header.groups())
    pixels = raw[header.end():]
    if len(pixels) != width * height * 3:
        raise ValueError('Truncated wood fixture capture')
    rows = frame['rows']
    result = {'fixture': 'wood_cap_self_shadow', 'sample_radius_px': 1,
              'luminance_space': 'display RGB weighted by 0.2126/0.7152/0.0722',
              'minimum_neighbor_luminance': 0.05, 'minimum_skinny_neighbor_ratio': 0.30,
              'panels': [], 'skipped_panels': [], 'status': 'passed'}
    for index, name in ((0, 'top'), (1, 'bottom')):
        translation, scale = rows[33 + 40 * index], rows[34 + 40 * index]
        samples = {}
        for label, local in (('skinny', [-0.30, 0.064, 0.004]),
                             ('neighbor', [-0.30, 0.078, 0.004])):
            world = [translation[axis] + local[axis] * scale[axis] for axis in range(3)]
            # The optical packet records this head-on orthographic camera's
            # physical half-extents, independent of logical/backing resolution.
            x = round((0.5 + world[0] / (2 * rows[2][3])) * width)
            y = round((0.5 - world[1] / (2 * rows[3][3])) * height)
            if x < 1 or y < 1 or x >= width - 1 or y >= height - 1:
                samples[label] = {'local_m': local, 'pixel': [x, y], 'offscreen': True}
                continue
            luminances = []
            for py in range(y - 1, y + 2):
                for px in range(x - 1, x + 2):
                    at = (py * width + px) * 3
                    red, green, blue = pixels[at:at + 3]
                    luminances.append((0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255)
            samples[label] = {'local_m': local, 'pixel': [x, y],
                              'median_luminance': statistics.median(luminances)}
        if any(sample.get('offscreen') for sample in samples.values()):
            result['skipped_panels'].append({'panel': name, 'reason': 'fixture point outside capture',
                                             'samples': samples})
            continue
        skinny, neighbor = (samples[label]['median_luminance'] for label in ('skinny', 'neighbor'))
        ratio = skinny / neighbor if neighbor > 0 else 0
        passed = neighbor > result['minimum_neighbor_luminance'] and ratio > result['minimum_skinny_neighbor_ratio']
        result['panels'].append({'panel': name, 'samples': samples, 'skinny_neighbor_ratio': ratio,
                                 'status': 'passed' if passed else 'failed'})
        if not passed:
            result['status'] = 'failed'
    if not result['panels']:
        result['status'] = 'not_exercised_all_samples_offscreen'
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-only', action='store_true', help='skip native window/device captures')
    parser.add_argument('--native-only', action='store_true',
                        help='resume a recorded binary and its matching assets; requires --resume-evidence')
    parser.add_argument('--resume-evidence', type=Path,
                        help='reuse a previous evidence directory; preserve its binary and asset snapshot')
    parser.add_argument('--skip-generate', action='store_true')
    parser.add_argument('--binary', type=Path,
                        help='fresh-run binary after the build; defaults to target/debug/wormwood')
    parser.add_argument('--backends', nargs='+', choices=('WebGPU', 'Vulkan', 'OpenGL'),
                        default=['WebGPU', 'Vulkan', 'OpenGL'])
    parser.add_argument('--frames', type=int, default=2)
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    if args.resume_evidence:
        args.resume_evidence = args.resume_evidence.resolve()
        if args.binary is not None:
            parser.error('--resume-evidence already chooses its recorded binary; omit --binary')
        args.native_only = True
        args.skip_generate = True
    if args.native_only and not args.resume_evidence:
        parser.error('--native-only requires --resume-evidence so binary, metadata and shaders stay matched')
    if args.source_only and args.native_only:
        parser.error('--source-only and --native-only are mutually exclusive')
    if not 2 <= args.frames <= 120:
        parser.error('--frames must be 2..120')
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S')
    out = (args.out or ROOT / 'output/checks' / ('user-styles-' + stamp)).resolve()
    out.mkdir(parents=True, exist_ok=False)
    report = {'evidence': str(out), 'steps': [], 'captures': {},
              'visual_review': 'pending', 'live_resize_interaction': 'not_exercised',
              'device_scope': 'desktop', 'packaged': False, 'uploaded': False,
              'animation_source': {'mode': 'finite_capture_fixed_dt', 'seconds_per_frame': 1,
                                   'environment': 'WORMWOOD_WORKSTATION_CAPTURE_DT'},
              'pose_comparisons': {}, 'rendered_fixtures': {}}
    before = None
    poses = {}
    source_env = os.environ.copy()
    source_env.pop('WORMWOOD_STYLE', None)
    source_env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(out / 'source'))

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, command, timeout=900, env=None):
        print(name, flush=True)
        started = time.monotonic()
        log = out / (name + '.log')
        if env is None:
            env = source_env.copy()
        try:
            run_checked([str(arg) for arg in command], cwd=ROOT, log=log,
                        timeout=timeout, env=env, heartbeat=30)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            report['steps'].append({'name': name, 'status': 'failed', 'error': str(error)})
            save()
            print(log.read_text(errors='replace')[-14000:], flush=True)
            raise
        report['steps'].append({'name': name, 'status': 'passed',
                                'seconds': round(time.monotonic() - started, 3)})
        save()

    try:
        if not args.skip_generate:
            run('generate-samples', [sys.executable, 'BuildSupport/Workstation/make_style_samples.py'], 30)
        before = source_hashes()
        (out / 'source-before.json').write_text(json.dumps(before, indent=2) + '\n')
        binary = out / 'wormwood-style-validation'
        assets = out / 'assets'
        if args.resume_evidence:
            hashes, provenance = resume_runtime(args.resume_evidence, assets, binary)
            report.update(provenance)
            report['asset_sha256'] = hashes
            report['binary_sha256'] = sha256(binary)
        else:
            for test in ('user_style', 'style_shader'):
                run('test-' + test, ['cargo', 'test', '--offline', '--locked', '--lib', test,
                    '--', '--test-threads=1'], 1200)
                assert re.search(r'running [1-9][0-9]* tests?', (out / ('test-' + test + '.log')).read_text()), \
                    'Focused test filter did not select any tests: ' + test
            for test in ('browser_', 'workstation_ssle_'):
                run('test-' + test, ['cargo', 'test', '--offline', '--locked', '--lib', test,
                    '--', '--test-threads=1'], 1200)
            run('desktop-build', ['cargo', 'build', '--offline', '--locked', '--bin', 'wormwood'], 1200)
            # Freeze the exact executable and data submitted to the native hosts.
            shutil.copy2(args.binary or ROOT / 'target/debug/wormwood', binary)
            report['binary_sha256'] = sha256(binary)
            run('intent', [binary, '--inspect-intent', '--json'], 30)
            intent = json.loads((out / 'intent.log').read_text())
            report['intent_summary'] = intent.get('summary')
            assert intent.get('summary', {}).get('active_structural_mismatches') == 0, report['intent_summary']
            report['intent_scope'] = 'structural only; behavioral and visual obligations remain separate'
            report['source_check_steps'] = list(report['steps'])
            report['asset_sha256'] = copy_runtime(assets)
        examples = assets / 'Art/AudioWorkstation/StyleExamples'
        # Extra generated variants change only the author-chosen resize mode.
        for source, slug, resize in [('wood', 'wood-contain', 'contain'),
                                     ('flat-ui', 'flat-ui-stretch', 'stretch')]:
            folder = examples / slug
            if not folder.exists():
                shutil.copytree(examples / source, folder)
                manifest = json.loads((folder / 'style.json').read_text())
                manifest['id'] = 'validation.' + slug
                manifest['name'] += ' (' + resize + ' validation)'
                manifest['models']['frame']['resize'] = resize
                (folder / 'style.json').write_text(json.dumps(manifest, indent=2) + '\n')
        report['validation_variant_sha256'] = {
            str(path.relative_to(assets)): sha256(path)
            for slug in ('wood-contain', 'flat-ui-stretch')
            for path in sorted((examples / slug).rglob('*')) if path.is_file()}
        if not args.source_only:
            cases = [(slug, backend, '960x640', 'landscape')
                     for slug in ('carved-metal', 'wood', 'flat-ui')
                     for backend in args.backends]
            portrait_backend = 'WebGPU' if 'WebGPU' in args.backends else args.backends[0]
            cases.extend((slug, portrait_backend, '480x800', 'portrait')
                         for slug in ('carved-metal', 'wood', 'flat-ui'))
            cases.append(('gem-baseline', args.backends[0], '960x640', 'landscape'))
            cases.extend((slug, args.backends[0], '960x640', 'landscape')
                         for slug in ('wood-contain', 'flat-ui-stretch'))
            for slug, backend, size, orientation in cases:
                name = '-'.join((slug, backend, orientation))
                destination = out / name
                destination.mkdir()
                env = os.environ.copy()
                env.update(WORMWOOD_ASSET_ROOT=str(assets), WORMWOOD_OUTPUT_ROOT=str(destination),
                           WORMWOOD_FULLSCREEN='0',
                           WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_WORKSTATION_GPU_TIMING='0',
                           WORMWOOD_WORKSTATION_SIZE=size, WORMWOOD_WORKSTATION_CAPTURE_DT='1')
                env.pop('WORMWOOD_STYLE', None)
                if slug != 'gem-baseline':
                    env['WORMWOOD_STYLE'] = str(examples / slug)
                run('native-' + name, [binary, '--workstation', backend, '--frames', args.frames], 240, env)
                result = native_summary(out / ('native-' + name + '.log'))
                assert result.get('capture_dt') == 1, 'Executable did not accept the fixed capture clock'
                assert result.get('end_reason') == 'frame_limit' and result.get('frames') == args.frames, result
                assert result.get('entities_remaining') == 0, 'Native session leaked entities'
                panels = result.get('panels', [])
                assert len(panels) == 4 and all(panel.get('visual_extension') == 1 for panel in panels), \
                    'Finite capture must show four fully settled panels: ' + str(panels)
                frame = json.loads((destination / 'workstation-optical-frame.json').read_text())
                rows = frame['rows']
                pose_key = slug + '-' + size
                panel_rows = rows[32:192]
                pose_digest = hashlib.sha256(json.dumps(panel_rows, separators=(',', ':')).encode()).hexdigest()
                if pose_key in poses:
                    prior_backend, prior_rows = poses[pose_key]
                    differing = [32 + i for i, (prior, current) in enumerate(zip(prior_rows, panel_rows))
                                 if prior != current]
                    report['pose_comparisons'][pose_key]['compared_backends'].append(backend)
                    if differing:
                        report['pose_comparisons'][pose_key]['mismatch'] = {
                            'backend': backend, 'reference_backend': prior_backend,
                            'differing_rows': differing}
                        save()
                        raise AssertionError('Panel pose mismatch across backends for ' + pose_key
                                             + ': rows ' + str(differing))
                else:
                    poses[pose_key] = backend, panel_rows
                    report['pose_comparisons'][pose_key] = {
                        'reference_backend': backend, 'compared_backends': [],
                        'rows': [32, 192], 'row_end_exclusive': True,
                        'excluded_fields': [], 'sha256': pose_digest}
                if slug == 'gem-baseline':
                    assert rows[245][0] == 0, 'Gem baseline unexpectedly selected a user style'
                else:
                    assert rows[245][0] == 1 and rows[245][2] == 1, 'User style/background packet absent'
                    assert rows[246][0] > 0 and rows[246][1] > 0, 'Imported geometry packet absent'
                    for panel in range(4):
                        assert rows[32 + 40 * panel][0] == 0, 'Analytic gem unexpectedly enabled'
                        assert rows[33 + 40 * panel][3] == 1, 'Imported panel instance absent'
                capture = ppm_to_png(destination / ('triangle-' + backend + '.ppm'),
                                     destination / 'style.png')
                if slug == 'wood':
                    fixture = wood_shadow_fixture(frame, destination / ('triangle-' + backend + '.ppm'))
                    report['rendered_fixtures'][name] = fixture
                    save()
                    assert fixture['status'] == 'passed', 'Rendered wood self-shadow fixture failed or was not exercised: ' + name
                    if orientation == 'landscape':
                        assert len(fixture['panels']) == 2, 'Both settled wood panels must exercise the self-shadow fixture'
                expected_width, expected_height = map(int, size.split('x'))
                assert capture['width'] == expected_width and capture['height'] == expected_height, capture
                capture.update({'style_package': None if slug == 'gem-baseline' else str(examples / slug),
                                'custom_geometry_rows': rows[246][:2],
                                'logical_viewport': frame.get('logical_viewport'),
                                'capture_dt': result['capture_dt'], 'panels': panels,
                                'requested_size': size, 'backend': backend,
                                'frame_json': str(destination / 'workstation-optical-frame.json')})
                report['captures'][name] = capture
                save()
        report['status'] = 'checks_finished_visual_review_pending'
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        raise
    finally:
        if before is not None:
            after = source_hashes()
            (out / 'source-after.json').write_text(json.dumps(after, indent=2) + '\n')
            changed = sorted(key for key in before.keys() | after.keys() if before.get(key) != after.get(key))
            report['sources_changed_during_run'] = changed
            if changed:
                report['source_evidence'] = 'changed during run; rerun source checks before claiming current-source validation'
        save()
        print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
