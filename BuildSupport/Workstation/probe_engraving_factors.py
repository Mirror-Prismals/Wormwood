#!/usr/bin/env python3
"""Finite shader-only A/B diagnostics using the verified native timeline fixture.

Does not build/install an app or alter production WGSL. GPU medians here are
static 640x480 diagnostic costs, never application FPS or motion evidence.
"""
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
from engraving_factor import transform

INPUT = ROOT / 'output/checks/ruby-timeline-native-20261031-224550-202605'
VARIANTS = ('off', 'metadata', 'cosine', 'navigation-zero', 'all')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def compare(a, b):
    images = []
    for path in (a, b):
        values = array.array('f')
        values.frombytes(path.read_bytes())
        if sys.byteorder != 'little':
            values.byteswap()
        if len(values) != 640 * 480 * 4 or not all(map(math.isfinite, values)):
            raise RuntimeError('Incomplete/nonfinite output: ' + str(path))
        images.append(values)
    maximum = square = 0.
    maximum_at = changed = above = alpha_changes = 0
    for i, (left, right) in enumerate(zip(*images)):
        if i % 4 == 3:
            alpha_changes += left != right
            continue
        error = abs(left-right)
        square += error*error
        changed += error != 0
        above += error > 0.00003
        if error > maximum:
            maximum, maximum_at = error, i
    return {'max_rgb_error': maximum, 'rgb_rmse': math.sqrt(square/(640*480*3)),
            'max_error_pixel': [maximum_at//4 % 640, maximum_at//4//640],
            'max_error_channel': maximum_at % 4, 'different_rgb_channels': changed,
            'channels_over_3e_minus5': above, 'alpha_channels_changed': alpha_changes,
            'timeline_cache_sample_sum': sum(images[1][3::4])}


def main():
    out = ROOT / 'output/checks' / ('engraving-factors-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    report = {'status': 'running', 'installed': False, 'native_fps_measured': False,
              'scope': 'Static 640x480 authentic timeline fixture; isolated arithmetic transforms. '
                       'Unchanged optical budgets, glyph rasters, plan, geometry and cache coverage.',
              'steps': [], 'cases': {}, 'source_evidence': str(INPUT/'report.json')}
    installed = ROOT / 'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine'
    inputs = [INPUT/name for name in ('timeline-probe', 'baseline.wgsl', 'candidate.wgsl',
              'baseline.frame.bin', 'candidate.frame.bin', 'baseline.plan.bin', 'candidate.plan.bin')]
    inputs += [Path(__file__).resolve(), Path(__file__).with_name('engraving_factor.py'), installed]
    before = {str(p): digest(p) for p in inputs}
    report['input_sha256'] = before

    def save():
        (out/'report.json').write_text(json.dumps(report, indent=2)+'\n')

    def run(label, argv, timeout=120):
        print(label, flush=True)
        env = {k:v for k,v in os.environ.items() if not k.startswith('WORMWOOD_')}
        log = out/(label+'.log')
        try:
            run_checked(list(map(str, argv)), cwd=ROOT, log=log, env=env, timeout=timeout)
        except BaseException:
            if log.exists():
                print(log.read_text(errors='replace')[-4000:], flush=True)
            raise
        report['steps'].append(label)
        save()

    try:
        verified = json.loads((INPUT/'report.json').read_text())
        if verified.get('status') != 'completed_staged_diagnostics' or not all(
                verified.get(k) is True for k in ('source_unchanged', 'original_fixture_unchanged',
                                                 'installed_engine_unchanged', 'staged_bundle_unchanged')):
            raise RuntimeError('Reference fixture validation was not complete')
        if shutil.disk_usage(ROOT).free < 512*1024**2:
            raise RuntimeError('Need 512MiB free for bounded shader diagnostics')
        run('test-transform', [sys.executable, ROOT/'BuildSupport/Workstation/test_engraving_factor.py'])
        validator_source = out/'validate.rs'
        validator_source.write_text('''fn main() {
    for path in std::env::args().skip(1) {
        let source=std::fs::read_to_string(&path).unwrap();
        let module=naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}: {}",path,e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all()).validate(&module).unwrap();
        for name in ["cs_ruby_response","cs_ruby_response_general","cs_ruby_resolve","fs_main"] {
            assert!(module.entry_points.iter().any(|e|e.name==name),"missing {name} in {path}");
        }
        println!("valid {}",path);
    }
}''')
        deps = ROOT/'target/release/deps'
        naga = max(deps.glob('libnaga-*.rlib'), key=lambda p:p.stat().st_mtime)
        report['validator_library'] = {'path': str(naga), 'sha256': digest(naga)}
        run('compile-validator', ['rustc', '--edition=2024', '-O', validator_source,
                                 '--extern', 'naga='+str(naga), '-L', 'dependency='+str(deps),
                                 '-o', out/'validate'])
        for family in ('baseline', 'candidate'):
            source = (INPUT/(family+'.wgsl')).read_text()
            for variant in VARIANTS:
                shader = out/(family+'-'+variant+'.wgsl')
                shader.write_text(transform(source, variant))
                run('naga-'+family+'-'+variant, [out/'validate', shader])
        # Each original is measured in the same new sequence, with a closing
        # repeat to expose drift. Cold and warm are independent workloads.
        for family in ('baseline', 'candidate'):
            for mode in ('warm', 'cold'):
                for variant in (*VARIANTS, 'off-repeat'):
                    shader_variant = 'off' if variant=='off-repeat' else variant
                    label = family+'-'+mode+'-'+variant
                    run('gpu-'+label, [INPUT/'timeline-probe', out/(family+'-'+shader_variant+'.wgsl'),
                        INPUT/(family+'.frame.bin'), INPUT/(family+'.plan.bin'), 640, 480,
                        out/label, 6, mode, 'resolve'], 300)
                    result = json.loads((out/(label+'.json')).read_text())
                    if result.get('status') != 'completed' or result.get('nonfinite_output_channels') != 0:
                        raise RuntimeError('Invalid GPU result: '+label)
                    original = out/(family+'-'+mode+'-off.rgba32f')
                    image = compare(original, out/(label+'.rgba32f'))
                    report['cases'][label] = {'result': result, 'appearance': image,
                                             'shader_sha256': digest(out/(family+'-'+shader_variant+'.wgsl'))}
                    save()
                    if image['channels_over_3e_minus5'] or image['alpha_channels_changed']:
                        raise RuntimeError('Appearance/cache coverage mismatch: '+label)
                    if family=='candidate' and image['timeline_cache_sample_sum']<=0:
                        raise RuntimeError('No actual timeline cache hits: '+label)
        report['status'] = 'completed_shader_diagnostics'
    except BaseException as error:
        report.update(status='failed_shader_diagnostics', error=str(error))
        raise
    finally:
        changed = [str(p) for p in inputs if not p.is_file() or digest(p)!=before[str(p)]]
        report['changed_inputs'] = changed
        if changed:
            report['status'] = 'failed_identity_check'
        save()
        print(json.dumps({'status':report['status'],'installed':False,'report':str(out/'report.json')}), flush=True)
        if changed:
            raise RuntimeError('Diagnostic input changed')


if __name__ == '__main__':
    main()
