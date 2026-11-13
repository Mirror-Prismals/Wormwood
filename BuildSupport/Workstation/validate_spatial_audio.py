"""Finite source checks and optional native captures for spatial audio spheres.

No installation, packaging, release upload or hardware audio playback. Native
captures require visual review; live playback and gestures remain separate.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked


def source_hashes():
    files = set()
    for directory in ('Workstation', 'Audio', 'BaseSystem', 'Host', 'Systems',
                      'Platform', 'Procedures/Shaders'):
        files.update(p for p in (ROOT / directory).rglob('*') if p.is_file()
                     and p.suffix in ('.rs', '.json', '.glsl', '.wgsl', '.frag',
                                      '.vert', '.py', '.mm', '.cpp', '.h'))
    files.update(ROOT / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'lib.rs'))
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(files) if p.is_file()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--native', action='store_true')
    args = parser.parse_args()
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S')
    out = ROOT / 'output/checks' / ('spatial-audio-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    before = source_hashes()
    (out / 'source-before.json').write_text(json.dumps(before, indent=2) + '\n')
    report = {'checks': [], 'native': {}, 'visual_review': 'pending',
              'hardware_audio': 'not_exercised', 'live_gestures': 'not_exercised',
              'evidence': str(out)}

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, argv, timeout=900, env=None):
        print(name, flush=True)
        started = time.monotonic()
        log = out / (name + '.log')
        try:
            run_checked([str(x) for x in argv], cwd=ROOT, log=log,
                        timeout=timeout, env=env, heartbeat=30)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            report['checks'].append({'name': name, 'status': 'failed', 'error': str(error)})
            save()
            print(log.read_text(errors='replace')[-18000:], flush=True)
            raise
        record = {'name': name, 'status': 'passed',
                  'seconds': round(time.monotonic() - started, 3)}
        if len(argv) > 1 and argv[0] == 'cargo' and argv[1] == 'test':
            count = sum(int(n) for n in re.findall(r'test result: ok\. (\d+) passed', log.read_text()))
            assert count > 0, f'{name} matched no tests'
            record['tests_passed'] = count
        report['checks'].append(record)
        save()

    try:
        # Generated frontends are already produced as part of the edit. Validate
        # those exact files instead of rewriting them during the build.
        for frontend in ('gl', 'ios'):
            run('shader-' + frontend, ['glslangValidator', '-S', 'frag',
                f'Procedures/Shaders/workstation.{frontend}.frag'], 90)
        run('shader-vulkan', ['glslangValidator', '-V', 'Procedures/Shaders/workstation.frag',
            '-o', out / 'workstation.spv'], 90)
        for name, test_filter in (
            ('source-analysis-tests', 'workstation::source_analysis_tests'),
            ('spatial-controller-tests', 'workstation::spatial_mix::tests'),
            ('sphere-packet-tests', 'workstation::audio_spheres::tests'),
            ('playback-regression-tests', 'workstation::playback'),
            ('sphere-host-tests', 'workstation_source_spheres'),
            ('workstation-lifecycle-test', 'workstation_registered_lifecycle'),
        ):
            run(name, ['cargo', 'test', '--offline', '--locked', '--lib', test_filter,
                '--', '--test-threads=1'], 1200)
        run('transport-tests', ['cargo', 'test', '--offline', '--locked', '--lib',
            'audio::transport::tests', '--', '--test-threads=1'], 600)
        run('source-observation-cache-tests', ['cargo', 'test', '--offline', '--locked', '--lib',
            'source_observation', '--', '--test-threads=1'], 600)
        run('desktop-build', ['cargo', 'build', '--offline', '--locked', '--bin', 'wormwood'], 1200)
        binary = ROOT / 'target/debug/wormwood'
        run('intent', [binary, '--inspect-intent', '--json'], 30)
        intent = json.loads((out / 'intent.log').read_text())
        report['intent_summary'] = intent.get('summary')
        sphere_behaviors = [b for b in intent.get('behaviors', [])
                            if any(key in b['id'] for key in ('sphere', 'source_stereo_observations', 'playback_source_observations'))]
        report['sphere_intent'] = sphere_behaviors
        assert sphere_behaviors, 'Missing spatial audio intent bindings'
        for behavior in sphere_behaviors:
            for check in behavior.get('checks', []):
                assert not check.get('missing_targets'), check
            assert all(s.get('status') == 'present' for s in behavior.get('sources', [])), behavior
        if args.native:
            for backend in ('WebGPU', 'Vulkan', 'OpenGL'):
                destination = out / backend
                destination.mkdir()
                env = os.environ.copy()
                env.update(WORMWOOD_OUTPUT_ROOT=str(destination),
                           WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_FULLSCREEN='0',
                           WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_WORKSTATION_GPU_TIMING='0')
                run('native-' + backend, [binary, '--check-audio-spheres', backend], 240, env)
                evidence = json.loads((destination / f'audio-spheres-{backend}.json').read_text())
                assert evidence['entities_remaining'] == 0, evidence
                assert evidence['spatial_error'] is None, evidence
                sources = evidence['spatial_mix']['sources']
                assert len(sources) == 3 and all(s['available'] and s['fresh'] for s in sources), evidence
                frame = json.loads((destination / 'workstation-optical-frame.json').read_text())
                rows = frame['rows']
                assert len(rows) == 49408 and rows[240][:3] == [3, 48128, 4], rows[240]
                ppm = destination / f'triangle-{backend}.ppm'
                magic, size, maximum, pixels = ppm.read_bytes().split(b'\n', 3)
                width, height = map(int, size.split())
                assert magic == b'P6' and maximum == b'255' and len(pixels) == width * height * 3
                assert max(pixels) > min(pixels), 'Empty/constant native capture'
                run('preview-' + backend, ['/usr/bin/sips', '-s', 'format', 'png', ppm,
                    '--out', destination / 'audio-spheres.png'], 30)
                report['native'][backend] = {'process': 'passed', 'width': width, 'height': height,
                                            'submitted_spheres': 3, 'visual_review': 'pending'}
                save()
        report['status'] = 'checks_finished_visual_review_pending'
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        raise
    finally:
        after = source_hashes()
        (out / 'source-after.json').write_text(json.dumps(after, indent=2) + '\n')
        report['sources_changed_during_run'] = sorted(k for k in before.keys() | after.keys()
                                                    if before.get(k) != after.get(k))
        save()
        print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
