"""Bounded local evidence for the authored theatre and shared DAW session."""
from pathlib import Path
import json
import os
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
sys.path.insert(0, str(ROOT / 'BuildSupport/Phone'))
from checked_process import run_checked
from validate_phone import theatre_capture


def main():
    out = ROOT / 'output' / ('theatre-port-' + time.strftime('%Y%m%d-%H%M%S'))
    out.mkdir(parents=True)
    report = {'checks': [], 'native_visual_review': 'pending',
              'hardware_visual_review': 'pending', 'output': str(out)}

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, argv, timeout, env=None):
        print(name, flush=True)
        log = out / (name + '.log')
        started = time.monotonic()
        try:
            run_checked([str(x) for x in argv], cwd=ROOT, log=log,
                        timeout=timeout, env=env, heartbeat=30)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            report['checks'].append({'name': name, 'status': 'failed', 'error': str(error)})
            save()
            print(log.read_text(errors='replace')[-14000:], flush=True)
            raise
        report['checks'].append({'name': name, 'status': 'passed',
                                'seconds': round(time.monotonic() - started, 3)})
        save()
        return log

    try:
        run('blender-hardware-preview', ['/Applications/Blender.app/Contents/MacOS/Blender',
            '-b', ROOT / 'Art/Theatre/TheatreHardware.blend', '-f', '1'], 300)
        run('build-and-inspect', ['cargo', 'run', '--offline', '--locked', '--',
            '--inspect-intent'], 1200)
        binary = ROOT / 'target/debug/wormwood'
        log = run('intent-json', [binary, '--inspect-intent', '--json'], 30)
        intent = json.loads(log.read_text())
        report['intent_summary'] = intent['summary']
        assert intent['summary']['active_structural_mismatches'] == 0, intent['summary']
        for name, pattern in [
            ('venue', 'theatre::tests'),
            ('cabinet-dsp', 'audio::line_array::tests'),
            ('transport', 'audio::transport::tests'),
            ('acoustic-solids', 'audio::geometry::tests'),
            ('wavefronts', 'audio_ray_visualizer_system::tests'),
            ('computer-focus', 'computer_focus'),
            ('computer-recovery', 'failed_computer_presentation'),
            ('workstation', 'workstation'),
        ]:
            log = run('test-' + name, ['cargo', 'test', '--offline', '--locked',
                '--lib', pattern, '--', '--test-threads=1'], 1200)
            found = re.findall(r'test result: ok\. (\d+) passed;', log.read_text())
            assert found and sum(map(int, found)) > 0, 'No tests selected: ' + pattern
            report['checks'][-1]['passed_tests'] = sum(map(int, found))
            save()
        destination = out / 'native'
        destination.mkdir()
        env = os.environ.copy()
        env.update(WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_ASSET_ROOT=str(ROOT),
                   WORMWOOD_FULLSCREEN='0')
        env.pop('WORMWOOD_STOP_FILE', None)
        run('native-theatre', [binary, '--interactive', 'planet_surface', 'OpenGL',
            '--close-after-frames', '6'], 600, env)
        report['native_session'] = theatre_capture(destination)
        ppm = destination / 'triangle-OpenGL.ppm'
        magic, size, maximum, pixels = ppm.read_bytes().split(b'\n', 3)
        width, height = map(int, size.split())
        assert magic == b'P6' and maximum == b'255' and len(pixels) == width * height * 3
        assert max(pixels) > min(pixels), 'Constant native capture'
        run('native-preview', ['/usr/bin/sips', '-s', 'format', 'png', ppm,
            '--out', destination / 'theatre-spawn.png'], 30)
        report['native_capture'] = {'width': width, 'height': height,
                                    'path': str(destination / 'theatre-spawn.png')}
        report['status'] = 'checks_passed_visual_review_pending'
        save()
        print(json.dumps(report, indent=2), flush=True)
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        save()
        raise


if __name__ == '__main__':
    main()
