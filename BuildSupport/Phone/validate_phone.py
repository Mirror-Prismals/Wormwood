"""Finite local phone/theatre validation; the parent launches this as one job.

Produces native captures and test evidence, without packaging or uploading.
Native audio is exercised; acoustic listening and physical input remain separate.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import re
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked

SCREENS = ('home', 'flashlight', 'camera', 'meter', 'weather')
TAIL_BYTES = 18000
MAX_IMAGE_BYTES = 256 * 1024 * 1024


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()


def source_hashes():
    """Relevant source/assets only; never walk build output or vendored code."""
    names = (
        'Cargo.toml', 'Cargo.lock', 'build.rs', 'lib.rs',
        'Host/Host.rs', 'Host/Intent.rs', 'Platform/Desktop.rs',
        'Audio/chuck.rs', 'Audio/LineArray.rs', 'Audio/Transport.rs',
        'Audio/geometry.rs', 'Audio/acoustic.rs', 'Audio/mod.rs',
        'Physics/matter/game.rs',
        'BaseSystem/LayerFive/PhoneSystem.rs',
        'BaseSystem/LayerFive/ApplicationSystem.rs',
        'BaseSystem/LayerFive/PlanetNavigationSystem.rs',
        'BaseSystem/LayerFive/EnvironmentAudioSystem.rs',
        'BaseSystem/LayerFive/AudioWorkstationSystem.rs',
        'BaseSystem/LayerFive/PlayerControls.rs',
        'BaseSystem/LayerFive/mod.rs',
        'BaseSystem/LayerFour/PhoneRenderSystem.rs',
        'BaseSystem/LayerFour/AppearanceSystem.rs',
        'BaseSystem/LayerFour/PlayerRigSystem.rs',
        'BaseSystem/LayerFour/Projection.rs',
        'BaseSystem/LayerFour/AudioRayVisualizerSystem.rs',
        'BaseSystem/LayerFour/mod.rs',
        'BaseSystem/LayerTwo/TheatreSystem.rs',
        'BaseSystem/LayerTwo/WorkstationDataSystem.rs',
        'BaseSystem/LayerTwo/PhysicsSystem.rs',
        'BaseSystem/LayerOne/GraphicsConnectionSystem.rs',
        'BaseSystem/LayerOne/JackAudio.rs',
        'BaseSystem/LayerOne/IOSAudio.rs',
        'BaseSystem/registry.json',
        'Systems/PhoneSystem.json', 'Systems/PhoneRenderSystem.json',
        'Systems/AppearanceSystem.json', 'Systems/PlanetNavigationSystem.json',
        'Systems/ApplicationSystem.json', 'Systems/PhysicsSystem.json',
        'Systems/EnvironmentAudioSystem.json', 'Systems/TheatreSystem.json',
        'Systems/AudioWorkstationSystem.json',
        'Procedures/functionregistry.json', 'Procedures/systems.json',
        'Procedures/Assets/Phone/phone_mesh.json',
        'Render/window.rs', 'Render/ios_window.rs', 'Render/ios_pending.rs',
        'Render/ios.rs', 'Render/Scene.rs',
        'Workstation/TimelineScene.rs', 'Workstation/Playback.rs',
        'BuildSupport/checked_process.py',
        'BuildSupport/Theatre/validate_theatre.py',
        'BuildSupport/Phone/validate_phone.py',
    )
    paths = {ROOT / name for name in names}
    for directory, suffixes in (
        ('Phone', {'.rs'}),
        ('Theatre', {'.rs', '.json'}),
        ('Art/Phone', {'.py', '.blend', '.glb', '.md'}),
    ):
        paths.update(path for path in (ROOT / directory).rglob('*')
                     if path.is_file() and path.suffix in suffixes)
    # Keep missing explicit paths in the snapshot so temporary concurrent moves
    # (including TimelineScene.rs) are distinguishable from the checked revision.
    return {str(path.relative_to(ROOT)): digest(path) if path.is_file() else None
            for path in sorted(paths)}


def read_json(path):
    if path.stat().st_size > 32 * 1024 * 1024:
        raise RuntimeError(f'JSON evidence exceeds its bound: {path}')
    return json.loads(path.read_text())


def tail(path):
    if not path.is_file():
        return '(no process log was created)'
    with path.open('rb') as stream:
        stream.seek(max(0, path.stat().st_size - TAIL_BYTES))
        return stream.read(TAIL_BYTES).decode('utf-8', errors='replace')


def png_info(path):
    with path.open('rb') as stream:
        if stream.read(8) != b'\x89PNG\r\n\x1a\n':
            raise RuntimeError(f'PNG preview missing or invalid: {path}')
    return {'path': str(path), 'bytes': path.stat().st_size, 'sha256': digest(path)}


def ppm_info(path):
    size = path.stat().st_size
    if size > MAX_IMAGE_BYTES:
        raise RuntimeError(f'Native image exceeds its bound: {path}')
    with path.open('rb') as stream:
        if stream.readline(80) != b'P6\n':
            raise RuntimeError(f'Not a native P6 image: {path}')
        dimensions = stream.readline(80).split()
        if len(dimensions) != 2:
            raise RuntimeError(f'Invalid native dimensions: {path}')
        width, height = map(int, dimensions)
        if not (1 <= width <= 16384 and 1 <= height <= 16384):
            raise RuntimeError(f'Native dimensions exceed the bound: {path}')
        if stream.readline(80) != b'255\n' or size != stream.tell() + width * height * 3:
            raise RuntimeError(f'Incomplete native RGB image: {path}')
        low, high = 255, 0
        pixels = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            low, high = min(low, min(chunk)), max(high, max(chunk))
            pixels.update(chunk)
    if low == high:
        raise RuntimeError(f'Constant native capture: {path}')
    return {'path': str(path), 'width': width, 'height': height, 'bytes': size,
            'pixel_sha256': pixels.hexdigest(), 'range': [low, high]}


def test_count(log):
    count = 0
    with log.open(errors='replace') as stream:
        for line in stream:
            match = re.search(r'test result: ok\. (\d+) passed;', line)
            if match:
                count += int(match.group(1))
    if count == 0:
        raise RuntimeError('Test filter selected no passing tests')
    return {'passed_tests': count}


def theatre_capture(destination):
    report_path = destination / 'interactive-OpenGL.json'
    evidence = read_json(report_path)
    if (evidence.get('mode') != 'interactive' or evidence.get('frames') != 6
            or evidence.get('entities_remaining') != 0
            or evidence.get('diagnostic_commands') is not False
            or evidence.get('flight', {}).get('medium') != 'planet'
            or evidence.get('flight', {}).get('moving') is not False):
        raise RuntimeError('Six-frame native planet session did not close cleanly')
    # ApplicationSystem returns at the frame bound, without a close_source field.
    # readback_verified is a triangle-center check, not a terrain-image assertion.
    drawn, pumps, steps = 0, 0, 0
    trace = destination / 'execution.jsonl'
    if trace.stat().st_size > 256 * 1024 * 1024:
        raise RuntimeError('Native theatre trace exceeds its bound')
    with trace.open() as stream:
        for line in stream:
            event = json.loads(line)
            if event.get('event') == 'window_pumped':
                if event.get('closed') is True:
                    raise RuntimeError('Native theatre window closed before its frame bound')
                pumps += 1
                drawn += event.get('drawn') is True
            elif (event.get('event') == 'finished'
                  and event.get('function') == 'PlanetNavigationSystem.Step'
                  and event.get('error') is None):
                steps += 1
    # Navigation emits environment_step only when the player changes; an idle
    # frame still completes Step and advances the scene. Native pumps may also
    # reuse an unchanged frame instead of drawing it again.
    if pumps != 6 or drawn < 1 or steps != 6:
        raise RuntimeError(f'Theatre session incomplete: {pumps} pumps, {drawn} draws, {steps} updates')
    return {'report': str(report_path), 'frames': 6, 'entities_remaining': 0,
            'drawn_frames': drawn, 'completed_navigation_steps': steps,
            'capture': ppm_info(destination / 'triangle-OpenGL.ppm')}


def native_environment(destination):
    destination.mkdir(parents=True, exist_ok=False)
    env = os.environ.copy()
    env.update(WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_ASSET_ROOT=str(ROOT),
               WORMWOOD_FULLSCREEN='0')
    # A stop-file from a separate user session must not affect this finite check.
    env.pop('WORMWOOD_STOP_FILE', None)
    return env


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / 'output/checks' / ('phone-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    before = source_hashes()
    (out / 'source-before.json').write_text(json.dumps(before, indent=2) + '\n')
    report = {'status': 'running', 'checks': [], 'native': {}, 'output': str(out),
              'visual_review': 'pending', 'physical_input_playtest': 'pending',
              'acoustic_listening': 'pending', 'ios_device_test': 'not_run',
              'packaged': False, 'uploaded': False}

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, argv, timeout, env=None, verify=None):
        command = [str(value) for value in argv]
        log = out / (name + '.log')
        record = {'name': name, 'argv': command, 'log': str(log),
                  'deadline_seconds': timeout, 'status': 'running'}
        report['checks'].append(record)
        save()
        print(name, flush=True)
        started = time.monotonic()
        try:
            run_checked(command, cwd=ROOT, log=log, timeout=timeout,
                        env=env, heartbeat=30)
            if verify:
                details = verify(log)
                if details:
                    record.update(details)
            record['status'] = 'passed'
        except BaseException as error:
            record.update(status='failed', error=f'{type(error).__name__}: {error}')
            print(tail(log), flush=True)
            raise
        finally:
            record['seconds'] = round(time.monotonic() - started, 3)
            save()
        return log

    save()
    print(f'Phone validation evidence: {out}', flush=True)
    try:
        preview = out / 'phone-product-0001.png'
        run('blender-phone-product', ['/Applications/Blender.app/Contents/MacOS/Blender',
            '-b', ROOT / 'Art/Phone/Phone.blend', '-o', out / 'phone-product-####',
            '-F', 'PNG', '-f', '1'], 600,
            verify=lambda _: {'preview': png_info(preview)})

        # Required source/contract inspection also produces the native binary.
        run('build-and-inspect', ['cargo', 'run', '--offline', '--locked', '--',
            '--inspect-intent'], 1200)
        binary = ROOT / 'target/debug/wormwood'

        def verify_intent(log):
            intent = read_json(log)
            summary = intent['summary']
            report['intent_summary'] = summary
            if summary.get('active_structural_mismatches') != 0:
                raise RuntimeError(f'Intent structural mismatches: {summary}')
            return {'active_structural_mismatches': 0}

        run('intent-json', [binary, '--inspect-intent', '--json'], 30, verify=verify_intent)
        for name, pattern in (
            ('phone-state-renderer-and-flashlight', 'phone'),
            ('listener-meter', 'listener_meter_'),
            ('tool-suspension', 'tool_suspension_checks'),
            # Preserve the existing theatre validation's regression coverage.
            ('venue', 'theatre::tests'),
            ('cabinet-dsp', 'audio::line_array::tests'),
            ('transport', 'audio::transport::tests'),
            ('acoustic-solids', 'audio::geometry::tests'),
            ('wavefronts', 'audio_ray_visualizer_system::tests'),
            ('computer-focus', 'computer_focus'),
            ('computer-recovery', 'failed_computer_presentation'),
            ('workstation', 'workstation'),
        ):
            run('test-' + name, ['cargo', 'test', '--offline', '--locked', '--lib',
                pattern, '--', '--test-threads=1'], 1200, verify=test_count)

        phone_output = out / 'native-phone'
        phone_env = native_environment(phone_output)

        def verify_phone(_):
            evidence_path = phone_output / 'phone-OpenGL/report.json'
            evidence = read_json(evidence_path)
            if evidence.get('complete') is not True or evidence.get('cleanup_errors'):
                raise RuntimeError(f'Native phone check incomplete: {evidence_path}')
            if evidence['entities_remaining'] != evidence['initial_entities']:
                raise RuntimeError('Native phone check leaked Entity ownership')
            captures = evidence.get('captures', [])
            if len(captures) != len(SCREENS) or {row['screen'] for row in captures} != set(SCREENS):
                raise RuntimeError('Native phone check did not capture all five apps')
            by_screen = {row['screen']: row for row in captures}
            if by_screen['camera']['phone'].get('camera_live') is not True:
                raise RuntimeError('Native phone camera was not live')
            if by_screen['meter']['phone']['meter'].get('available') is not True:
                raise RuntimeError('Native phone meter has no real listener samples')
            native_audio = evidence['environment_audio']['native']
            if native_audio.get('callbacks', 0) <= 0 or native_audio.get('samples_produced', 0) <= 0:
                raise RuntimeError('Native audio callback did not consume output')
            images = {}
            for screen in SCREENS:
                row = by_screen[screen]
                if row.get('native_drawn') is not True or row.get('fresh_readback') is not True:
                    raise RuntimeError(f'Native {screen} capture was not fresh')
                image = ppm_info(phone_output / 'phone-OpenGL' / f'phone-{screen}.ppm')
                if image['width'] != row['width'] or image['height'] != row['height']:
                    raise RuntimeError(f'Native {screen} dimensions differ from its report')
                images[screen] = image
            report['native']['phone'] = {'report': str(evidence_path), 'images': images,
                                         'visual_review': 'pending', 'native_audio': native_audio}
            return {'captured_screens': list(SCREENS)}

        run('native-phone-OpenGL', [binary, '--check-phone', 'OpenGL'], 600,
            env=phone_env, verify=verify_phone)
        for screen in SCREENS:
            ppm = phone_output / 'phone-OpenGL' / f'phone-{screen}.ppm'
            png = ppm.with_suffix('.png')
            run('preview-phone-' + screen, ['/usr/bin/sips', '-s', 'format', 'png',
                ppm, '--out', png], 30, verify=lambda _, path=png: {'preview': png_info(path)})
            report['native']['phone']['images'][screen]['preview'] = str(png)

        theatre_output = out / 'native-theatre'
        theatre_env = native_environment(theatre_output)

        def verify_theatre(_):
            evidence = theatre_capture(theatre_output)
            report['native']['theatre'] = {'image': evidence['capture'],
                'report': evidence['report'], 'visual_review': 'pending'}
            return evidence

        run('native-theatre-OpenGL', [binary, '--interactive', 'planet_surface', 'OpenGL',
            '--close-after-frames', '6'], 600, env=theatre_env, verify=verify_theatre)
        png = theatre_output / 'theatre-spawn.png'
        run('preview-theatre', ['/usr/bin/sips', '-s', 'format', 'png',
            theatre_output / 'triangle-OpenGL.ppm', '--out', png], 30,
            verify=lambda _: {'preview': png_info(png)})
        report['native']['theatre']['image']['preview'] = str(png)
        report['status'] = 'checks_passed_visual_review_pending'
    except BaseException as error:
        report['status'] = 'failed'
        report['error'] = f'{type(error).__name__}: {error}'
        raise
    finally:
        try:
            after = source_hashes()
            (out / 'source-after.json').write_text(json.dumps(after, indent=2) + '\n')
            changed = sorted(name for name in before.keys() | after.keys()
                             if before.get(name) != after.get(name))
            report['sources_changed_during_run'] = changed
            report['single_source_revision'] = not changed
            if changed and report['status'] == 'checks_passed_visual_review_pending':
                report['status'] = 'checks_passed_concurrent_edits_review_required'
        except BaseException as error:
            report['source_hash_error'] = f'{type(error).__name__}: {error}'
            report['single_source_revision'] = False
            report['status'] = 'failed'
            raise
        finally:
            save()
            print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
