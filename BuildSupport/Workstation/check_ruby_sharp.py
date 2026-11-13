#!/usr/bin/env python3
"""Measure full-resolution engraving and shared ruby lighting before integration.

Uses the approved scene and shader as immutable references. Produces images and
GPU measurements; never replaces the playable app or uploads a release.
"""
from pathlib import Path
import array
import datetime
import hashlib
import json
import math
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
import ruby_resolve
import ruby_sharp_text


def pixels(path):
    values = array.array('f')
    values.frombytes(path.read_bytes())
    if sys.byteorder != 'little':
        values.byteswap()
    if len(values) != 640 * 480 * 4 or not all(map(math.isfinite, values)):
        raise RuntimeError('Incomplete or nonfinite image: ' + str(path))
    return values


def difference(a, b):
    errors = [abs(a[i] - b[i]) for i in range(len(a)) if i % 4 != 3]
    return {'max_rgb_error': max(errors),
            'rgb_rmse': math.sqrt(sum(v*v for v in errors) / len(errors)),
            'alpha_count_mismatches': sum(a[i] != b[i] for i in range(3, len(a), 4))}


def main():
    reference = ROOT / 'output/checks/ruby-approved-20261031-144827-424473'
    metadata = json.loads((reference / 'response.json').read_text())
    fixture = Path(metadata['frame'])
    plan = reference / 'response.plan.bin'
    source = (reference / 'response.wgsl').read_text()
    out = ROOT / 'output/checks' / ('ruby-sharp-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    report = {'status': 'running', 'installed': False, 'native_integration': False,
              'reference': str(reference), 'steps': [], 'cases': {}}
    inputs = [fixture, plan, reference / 'response.wgsl',
              Path(__file__), Path(ruby_resolve.__file__), Path(ruby_sharp_text.__file__),
              ROOT / 'BuildSupport/Workstation/ruby_resolve_probe.cpp']
    digest = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    before = {str(p): digest(p) for p in inputs}
    report['input_sha256'] = before
    print('Evidence: ' + str(out), flush=True)

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, command, timeout=240):
        print(name, flush=True)
        log = out / (name + '.log')
        try:
            run_checked(list(map(str, command)), cwd=ROOT, log=log, timeout=timeout)
        except BaseException:
            print(log.read_text(errors='replace')[-7000:], flush=True)
            raise
        report['steps'].append(name)
        save()

    try:
        probe = out / 'ruby-resolve-probe'
        run('compile-probe', ['xcrun', 'clang++', '-std=c++17', '-O2', '-I/opt/homebrew/include',
            'BuildSupport/Workstation/ruby_resolve_probe.cpp', '-L/opt/homebrew/lib', '-lwgpu_native',
            '-Wl,-rpath,/opt/homebrew/lib', '-o', probe])
        sharp = ruby_sharp_text.transform(source)
        conservative = ruby_sharp_text.transform_components(source)
        shaders = {'approved': source, 'sharp': sharp, 'sharp-conservative': conservative,
                   'resolved': ruby_resolve.transform(source),
                   'resolved-sharp': ruby_resolve.transform(sharp)}
        for name, shader in shaders.items():
            (out / (name + '.wgsl')).write_text(shader)
        cases = [('approved', 'approved', 'warm', False),
                 ('full-resolution-reference', 'approved', 'baseline', False),
                 ('sharp', 'sharp', 'warm', False),
                 ('sharp-conservative', 'sharp-conservative', 'warm', False),
                 ('resolved', 'resolved', 'warm', True),
                 ('resolved-sharp', 'resolved-sharp', 'warm', True),
                 ('resolved-sharp-cold', 'resolved-sharp', 'cold', True),
                 ('approved-repeat', 'approved', 'warm', False)]
        for name, shader, mode, resolve in cases:
            command = [probe, out / (shader + '.wgsl'), fixture, plan, 640, 480, out / name, 6, mode]
            if resolve:
                command.append('resolve')
            run('gpu-' + name, command)
            result = json.loads((out / (name + '.json')).read_text())
            if result.get('nonfinite_output_channels', 0):
                raise RuntimeError('Nonfinite output: ' + name)
            report['cases'][name] = result
            save()
        images = {name: pixels(out / (name + '.rgba32f')) for name in report['cases']}
        for name, reference_name in [('resolved', 'approved'), ('resolved-sharp', 'sharp')]:
            comparison = difference(images[name], images[reference_name])
            report[name + '-equivalence'] = comparison
            if comparison['max_rgb_error'] > 0.00003 or comparison['alpha_count_mismatches']:
                raise RuntimeError('Shared lighting changed the reference pixels: ' + name)
        for name in ('sharp', 'sharp-conservative', 'resolved-sharp'):
            sharp_pixels, old, full = images[name], images['approved'], images['full-resolution-reference']
            restored = [i for i in range(0, len(old), 4) if old[i+3] > 0 and sharp_pixels[i+3] == 0]
            errors = [abs(sharp_pixels[i+c]-full[i+c]) for i in restored for c in range(3)]
            report[name + '-text-restoration'] = {
                'newly_full_resolution_pixels': len(restored),
                'max_rgb_error_vs_full_resolution': max(errors, default=0),
                'cached_samples': sum(sharp_pixels[3::4]),
                'limitation': 'Exact pixels are checked where both samples bypass caching; mask coverage needs visual review.'}
            if not restored or max(errors) > 0.00003:
                raise RuntimeError('Text fallback did not match full-resolution integration: ' + name)
        baseline = report['cases']['approved']['median_gpu_ms']
        report['speedup'] = {name: baseline / result['median_gpu_ms'] for name, result in report['cases'].items()}
        report['baseline_drift_percent'] = 100 * (report['cases']['approved-repeat']['median_gpu_ms'] / baseline - 1)
        for name in ('approved', 'full-resolution-reference', 'sharp', 'sharp-conservative', 'resolved-sharp'):
            run('image-' + name, ['/usr/bin/sips', '-s', 'format', 'png', out / (name + '.ppm'), '--out', out / (name + '.png')])
        if before != {str(p): digest(p) for p in inputs}:
            raise RuntimeError('Experiment inputs changed while measuring')
        report['status'] = 'completed_headless_pending_visual_review'
        save()
        print(json.dumps({'report': str(out / 'report.json'), 'speedup': report['speedup'],
                         'resolved_equivalence': report['resolved-equivalence'],
                         'text_restoration': report['resolved-sharp-text-restoration']}), flush=True)
    except BaseException as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        save()
        raise


if __name__ == '__main__':
    main()
