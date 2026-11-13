#!/usr/bin/env python3
"""Build the live combined checkout into a separate, unpromoted macOS app."""
from pathlib import Path
import datetime
import json
import os
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
sys.path.insert(0, str(ROOT / 'BuildSupport/Phone'))
from checked_process import run_checked
from validate_phone import digest, read_json, ppm_info, png_info, tail, test_count, theatre_capture

FILTERS = ('workstation', 'phone', 'listener_meter_', 'tool_suspension_checks',
           'collision_bounds_checks', 'theatre::tests', 'audio::line_array::tests',
           'audio::transport::tests', 'audio::geometry::tests',
           'audio_ray_visualizer_system::tests', 'computer_focus',
           'failed_computer_presentation')
SCREENS = ('home', 'flashlight', 'camera', 'meter', 'weather')


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def build_input(name):
    p = Path(name)
    if not p.parts or p.is_absolute() or '..' in p.parts:
        return False
    if p.parts[0] in ('output', 'target', 'Saves', '.git', '.codex', '.agents'):
        return False
    if '__pycache__' in p.parts or p.suffix == '.pyc':
        return False
    # Procedures include runtime images/audio: do not filter their extensions.
    if p.parts[0] in ('Procedures', 'Systems', 'SimulationWorkshop'):
        return True
    if p.parts[0] == 'Art':
        if name.startswith('Art/AudioWorkstation/CardinalThemes/Reference/'):
            return False
        return p.suffix.lower() in ('.json', '.bin', '.glb') or (name.startswith('Art/AudioWorkstation/CardinalThemes/') and p.suffix in ('.rgba32f','.rgba8'))
    if name.startswith('BuildSupport/Licenses/') or p.name in ('LICENSE', 'THIRD_PARTY_NOTICES.md'):
        return True
    if p.suffix.lower() in ('.blend', '.blend1', '.png', '.jpg', '.jpeg', '.webp',
                            '.gif', '.ppm', '.exr', '.wav', '.mp3', '.ogg', '.flac',
                            '.aiff', '.mp4', '.mov', '.pdf'):
        return False
    if 'native' in p.parts or 'vendor' in p.parts or name.startswith('Platform/Vst2Compat/'):
        return True
    return (p.name in ('CMakeLists.txt', 'Makefile', 'GNUmakefile')
            or p.suffix.lower() in ('.rs', '.c', '.cc', '.cpp', '.cxx', '.h', '.hh',
                '.hpp', '.hxx', '.inl', '.inc', '.m', '.mm', '.metal', '.glsl',
                '.wgsl', '.vert', '.frag', '.geom', '.comp', '.spv', '.ck', '.py',
                '.sh', '.cmake', '.toml', '.lock', '.json', '.yaml', '.yml',
                '.a', '.dylib', '.so', '.def', '.rc', '.plist'))


def tree_hashes(directory):
    result = {}
    for path in sorted(directory.rglob('*')):
        name = str(path.relative_to(directory))
        if path.is_symlink():
            result[name] = 'symlink:' + os.readlink(path)
        elif path.is_file():
            result[name] = digest(path)
    return result


def clean_env():
    env = os.environ.copy()
    for key in list(env):
        if key.startswith(('WORMWOOD_WORKSTATION_', 'WORMWOOD_TEST_', 'WORMWOOD_DIAGNOSTIC_')) or key in (
                'WORMWOOD_STYLE', 'WORMWOOD_STOP_FILE', 'WORMWOOD_OUTPUT_ROOT',
                'WORMWOOD_ASSET_ROOT', 'WORMWOOD_FULLSCREEN', 'WORMWOOD_PERFORMANCE'):
            env.pop(key, None)
    env.update(CARGO_TARGET_DIR=str(ROOT / 'target'), WORMWOOD_FULLSCREEN='0',
               WORMWOOD_PERFORMANCE='0')
    return env


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / 'output/checks' / ('combined-build-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    report = {'status': 'running', 'output': str(out), 'checks': [], 'failures': [],
              'installed': False, 'promoted': False, 'visual_review': 'pending'}
    before = None
    bundle_before = None
    app = None
    env = clean_env()
    env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(out / 'build-runtime'))

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def check(name, action):
        record = {'name': name, 'status': 'running'}
        report['checks'].append(record)
        started = time.monotonic()
        try:
            value = action()
            if value:
                record['evidence'] = value
            record['status'] = 'passed'
            return True
        except Exception as error:
            record.update(status='failed', error=f'{type(error).__name__}: {error}')
            report['failures'].append(name)
            print(f'{name}: {error}', flush=True)
            return False
        finally:
            record['seconds'] = round(time.monotonic() - started, 3)
            save()

    def run(name, argv, timeout=600, process_env=None, verify=None):
        command = [str(x) for x in argv]
        log = out / (name + '.log')
        def action():
            print(name, flush=True)
            try:
                run_checked(command, cwd=ROOT, log=log, timeout=timeout,
                            env=env if process_env is None else process_env, heartbeat=30)
                details = verify(log) if verify else None
                return {'argv': command, 'log': str(log), 'deadline_seconds': timeout,
                        'verification': details}
            except Exception:
                print(tail(log), flush=True)
                raise
        return check(name, action)

    def source_manifest(label):
        log = out / (label + '-git-files.log')
        require(run(label + '-git-files', ['git', 'ls-files', '--cached', '--others',
                    '--exclude-standard', '-z'], 30), 'Cannot enumerate combined source inputs')
        names = {os.fsdecode(n) for n in log.read_bytes().split(b'\0') if n}
        result = {name: digest(ROOT / name) if (ROOT / name).is_file() else None
                  for name in sorted(names) if build_input(name)}
        require('Cargo.toml' in result and 'lib.rs' in result, 'Git source manifest is not rooted in Wormwood')
        (out / (label + '.json')).write_text(json.dumps(result, indent=2) + '\n')
        return result

    def case_env(name, assets):
        destination = out / name
        destination.mkdir(exist_ok=False)
        case = clean_env()
        case.update(WORMWOOD_ASSET_ROOT=str(assets), WORMWOOD_OUTPUT_ROOT=str(destination))
        return destination, case

    def verify_intent(log):
        summary = read_json(log)['summary']
        require(summary.get('active_structural_mismatches') == 0, f'Contract mismatches: {summary}')
        report['intent_summary'] = summary
        return summary

    def capture_preview(name, ppm):
        # Every case starts in a new directory, so this cannot reuse an old frame.
        if not check(name + '-readback', lambda: ppm_info(ppm)):
            return
        png = ppm.with_suffix('.png')
        run(name + '-png', ['/usr/bin/sips', '-s', 'format', 'png', ppm, '--out', png],
            30, verify=lambda _: png_info(png))

    def workstation_report(destination, kind, backend):
        prefix = {'controls': 'workstation-controls', 'arrangement': 'arrangement',
                  'bubbles': 'bubble-piano-roll', 'spheres': 'audio-spheres'}[kind]
        value = read_json(destination / f'{prefix}-{backend}.json')
        require(value.get('frames') == 4 and value.get('end_reason') == 'frame_limit',
                f'{kind} did not finish four native frames')
        require(value.get('entities_remaining') == 0, f'{kind} leaked Entities')
        require(all(value.get(k) is None for k in ('editor_error', 'arrangement_error', 'spatial_error')),
                f'{kind} reports an editor/arrangement/spatial error')
        rows = read_json(destination / 'workstation-optical-frame.json')['rows']
        require(len(rows) == 49408, f'{kind} optical packet size differs')
        if kind == 'controls':
            require(value.get('complete') is True and value.get('mixer_controls_visible') is True
                    and value.get('controls_mixer') is True and value.get('controls_settled') is True,
                    'Authored mixer controls were not captured')
            require(all(rows[192 + i*2][3] > 0 for i in range(10)), 'Transport/mixer instance hidden')
        elif kind == 'arrangement':
            require(rows[248][0] >= 2, 'Arrangement timeline instances missing')
        elif kind == 'bubbles':
            require(value['piano_roll'].get('open') is True and value['piano_roll'].get('grid_visible') is False
                    and rows[234][0] == 4, 'Four bubble notes were not submitted')
        else:
            sources = value['spatial_mix']['sources']
            require(sum(s.get('available') is True and s.get('fresh') is True for s in sources) >= 3,
                    'Spatial mixer did not expose three fresh PCM observations')
            require(rows[240][:3] == [3, 48128, 4], 'Sphere packet header missing')
        image = ppm_info(destination / f'triangle-{backend}.ppm')
        return {'session_report': str(destination / f'{prefix}-{backend}.json'), 'capture': image}

    save()
    print(f'Combined build evidence: {out}', flush=True)
    try:
        run('generate-workstation', [sys.executable, ROOT / 'Procedures/Shaders/generate_workstation.py'], 60)
        before = source_manifest('source-before')
        run('build-and-inspect', ['cargo', 'run', '--offline', '--locked', '--', '--inspect-intent'], 1200)
        run('intent-json', [ROOT / 'target/debug/wormwood', '--inspect-intent', '--json'], 30, verify=verify_intent)
        # A failed regression is retained as a failed gate, but does not suppress
        # packaging or independent native evidence from the same current tree.
        for index, pattern in enumerate(FILTERS):
            run(f'test-{index:02d}-{pattern.replace("::", "-")}',
                ['cargo', 'test', '--offline', '--locked', '--lib', pattern, '--', '--test-threads=1'],
                1200, verify=test_count)
        package = out / 'package'
        project = read_json(ROOT / 'Project.json')
        name = project['name']
        require(isinstance(name, str) and name and Path(name).name == name, 'Invalid application package name')
        app = package / (name + '.app')
        report['app'] = str(app)
        packaged = run('package-mac', [sys.executable, ROOT / 'BuildSupport/macOS/build_app.py',
                        '--output-dir', package], 1200)
        report['package_built'] = packaged
        if packaged:
            assets = app / 'Contents/Resources/EngineAssets'
            engine = app / 'Contents/MacOS/wormwood-engine'
            bundle_before = tree_hashes(app)
            (out / 'bundle-before.json').write_text(json.dumps(bundle_before, indent=2) + '\n')

            def compare_assets():
                actual = {str(p.relative_to(assets)): digest(p) for p in sorted(assets.rglob('*')) if p.is_file()}
                mismatches = []
                for relative, sha in actual.items():
                    source = ('BuildSupport/Licenses/' + relative[len('ThirdPartyLicenses/'):]
                              if relative.startswith('ThirdPartyLicenses/') else relative)
                    path = ROOT / source
                    if not path.is_file() or digest(path) != sha:
                        mismatches.append(relative)
                    if source in before and before[source] != sha:
                        mismatches.append(relative + ': differs from build-input snapshot')
                for directory in ('Procedures', 'Systems', 'SimulationWorkshop', 'Platform/Vst2Compat'):
                    for path in (ROOT / directory).rglob('*'):
                        if path.is_file() and '__pycache__' not in path.parts and path.suffix != '.pyc':
                            if str(path.relative_to(ROOT)) not in actual:
                                mismatches.append(str(path.relative_to(ROOT)) + ': missing from bundle')
                (out / 'bundle-assets.json').write_text(json.dumps(actual, indent=2) + '\n')
                require(not mismatches, f'Bundled source assets differ: {mismatches[:30]}')
                return {'matched_files': len(actual), 'manifest': str(out / 'bundle-assets.json')}

            check('bundle-assets-match-checkout', compare_assets)
            destination, native_env = case_env('startup', assets)
            def verify_startup(_):
                value = read_json(destination / 'startup.json')
                require(value.get('mode') == 'system_startup' and value.get('started') is True
                        and value.get('failure') is None, 'Packaged startup did not complete')
                return {'report': str(destination / 'startup.json')}
            run('packaged-startup', [engine], 600, native_env, verify_startup)
            for kind, backend in (('controls', 'OpenGL'), ('controls', 'Vulkan'), ('controls', 'WebGPU'),
                                  ('arrangement', 'OpenGL'), ('bubbles', 'OpenGL'), ('spheres', 'OpenGL')):
                case = kind + '-' + backend
                destination, native_env = case_env(case, assets)
                flag = {'controls': '--check-workstation-controls', 'arrangement': '--check-arrangement',
                        'bubbles': '--check-bubble-notes', 'spheres': '--check-audio-spheres'}[kind]
                run('native-' + case, [engine, flag, backend], 600, native_env,
                    lambda _, d=destination, k=kind, b=backend: workstation_report(d, k, b))
                capture_preview(case, destination / f'triangle-{backend}.ppm')

            destination, native_env = case_env('phone-OpenGL', assets)
            def verify_phone(_):
                value = read_json(destination / 'phone-OpenGL/report.json')
                require(value.get('complete') is True and not value.get('cleanup_errors')
                        and value['entities_remaining'] == value['initial_entities'], 'Phone check incomplete')
                captures = value['captures']
                require(len(captures) == 5 and {v['screen'] for v in captures} == set(SCREENS), 'Phone screens missing')
                by_screen = {v['screen']: v for v in captures}
                require(by_screen['camera']['phone'].get('camera_live') is True, 'Selfie not active')
                require(by_screen['meter']['phone']['meter'].get('available') is True, 'Actual listener meter unavailable')
                native = value['environment_audio']['native']
                require(native.get('callbacks', 0) > 0 and native.get('samples_produced', 0) > 0, 'Audio callback did not run')
                for capture in captures:
                    require(capture.get('native_drawn') is True and capture.get('fresh_readback') is True, 'Stale phone capture')
                    image = ppm_info(destination / 'phone-OpenGL' / f'phone-{capture["screen"]}.ppm')
                    require(image['width'] == capture['width'] and image['height'] == capture['height'], 'Phone image dimensions differ')
                return {'report': str(destination / 'phone-OpenGL/report.json'), 'screens': list(SCREENS)}
            run('native-phone-OpenGL', [engine, '--check-phone', 'OpenGL'], 600, native_env, verify_phone)
            for screen in SCREENS:
                capture_preview('phone-' + screen, destination / 'phone-OpenGL' / f'phone-{screen}.ppm')

            destination, native_env = case_env('theatre-OpenGL', assets)
            def verify_theatre(_):
                return theatre_capture(destination)
            run('native-theatre-OpenGL', [engine, '--interactive', 'planet_surface', 'OpenGL', '--close-after-frames', '6'],
                600, native_env, verify_theatre)
            capture_preview('theatre-OpenGL', destination / 'triangle-OpenGL.ppm')
            run('codesign-deep-strict', ['/usr/bin/codesign', '--verify', '--deep', '--strict', app], 60)
    except Exception as error:
        report['failures'].append('workflow')
        report['workflow_error'] = f'{type(error).__name__}: {error}'
    finally:
        def verify_sources():
            after = source_manifest('source-after')
            require(before is not None, 'No prebuild source manifest')
            changed = sorted(n for n in before.keys() | after.keys() if before.get(n) != after.get(n))
            report['sources_changed_during_run'] = changed
            require(not changed, f'Build inputs changed during validation: {changed[:30]}')
            return {'input_files': len(after), 'changed': []}
        check('source-revision-unchanged', verify_sources)
        if bundle_before is not None:
            def verify_bundle():
                after = tree_hashes(app)
                (out / 'bundle-after.json').write_text(json.dumps(after, indent=2) + '\n')
                changed = sorted(n for n in bundle_before.keys() | after.keys() if bundle_before.get(n) != after.get(n))
                report['bundle_changed_during_native_checks'] = changed
                require(not changed, f'Packaged app changed during native checks: {changed[:30]}')
                return {'files': len(after), 'changed': []}
            check('bundle-unchanged-after-native', verify_bundle)
        report['status'] = 'failed_staged_app_retained' if report['failures'] else 'passed_staged_visual_review_pending'
        report['ready_for_visual_review'] = bool(report.get('package_built'))
        report['all_automated_gates_passed'] = not report['failures'] and bool(report.get('package_built'))
        report['ready_for_promotion'] = False  # Parent still owns visual review and promotion.
        save()
        print(json.dumps({'app': str(app) if app is not None and app.exists() else None,
                          'report': str(out / 'report.json'), 'status': report['status'],
                          'failures': report['failures'], 'installed': False, 'promoted': False}, indent=2), flush=True)
    return 1 if report['failures'] else 0


if __name__ == '__main__':
    sys.exit(main())
