"""Finite authored-asset, editor, contract and optional native bubble validation.

No installation, release upload, app packaging, or audio playback. All evidence
is written to a fresh output/checks directory. Native captures still require
human visual inspection; a successful process is not a visual quality verdict.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--native-only', action='store_true',
                        help='capture with an already validated desktop binary; skip source checks')
    parser.add_argument('--assets-only', action='store_true')
    parser.add_argument('--skip-export', action='store_true')
    args = parser.parse_args()
    if args.native_only and args.assets_only:
        parser.error('--native-only and --assets-only are mutually exclusive')
    if args.native_only:
        args.native = True
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S')
    out = ROOT / 'output/checks' / ('bubble-piano-roll-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    report = {'source_validation': [], 'native': {}, 'visual_review': 'pending',
              'audio_instrument': 'not_connected_by_this_change', 'evidence': str(out)}
    if args.native_only:
        report['source_check_scope'] = 'not_run; requires prior shared-build evidence'

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, argv, timeout=900, env=None):
        print(name, flush=True)
        start = time.monotonic()
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run([str(x) for x in argv], cwd=ROOT,
                                    stdout=log, stderr=subprocess.STDOUT,
                                    timeout=timeout, env=env)
        record = {'name': name, 'exit_code': result.returncode,
                  'seconds': round(time.monotonic() - start, 3)}
        report['source_validation'].append(record)
        save()
        if result.returncode:
            print((out / (name + '.log')).read_text(errors='replace')[-14000:])
            raise RuntimeError(f'{name} failed with exit {result.returncode}')

    try:
        if not args.skip_export and not args.native_only:
            run('asset-export', ['/Applications/Blender.app/Contents/MacOS/Blender',
                '--background', ROOT / 'Art/AudioWorkstation/BubbleNotes/BubbleNotes.blend',
                '--python-exit-code', '1', '--python', ROOT / 'BuildSupport/Blender/export_bubble_notes.py'], 120)
        if not args.native_only:
            run('shader-generation', [sys.executable, 'Procedures/Shaders/generate_workstation.py'], 20)
            for frontend in ('gl', 'ios'):
                run('shader-' + frontend, ['glslangValidator', '-S', 'frag',
                    f'Procedures/Shaders/workstation.{frontend}.frag'], 90)
            run('shader-vulkan', ['glslangValidator', '-V', 'Procedures/Shaders/workstation.frag',
                '-o', out / 'workstation.spv'], 90)
        if args.assets_only:
            report['status'] = 'assets_and_shaders_passed_build_pending'
            save()
            print(json.dumps(report, indent=2), flush=True)
            return
        binary = ROOT / 'target/debug/wormwood'
        if not args.native_only:
            run('workstation-tests', ['cargo', 'test', '--offline', '--locked', 'workstation',
                '--', '--test-threads=1'], 1200)
            run('desktop-build', ['cargo', 'build', '--offline', '--locked'], 1200)
            run('intent', [binary, '--inspect-intent', '--json'], 30)
            intent = json.loads((out / 'intent.log').read_text())
            report['intent_summary'] = intent.get('summary')
            report['status'] = 'source_checks_passed'
            save()
        if args.native:
            # Other agents may rebuild the shared target while these backends
            # run. Capture every backend with the same completed executable.
            native_binary = tempfile.TemporaryDirectory(prefix='bubble-native-')
            copied_binary = Path(native_binary.name) / 'wormwood'
            shutil.copy2(binary, copied_binary)
            report['native_binary_sha256'] = hashlib.sha256(copied_binary.read_bytes()).hexdigest()
            binary = copied_binary
            for backend in ('WebGPU', 'Vulkan', 'OpenGL'):
                destination = out / backend
                destination.mkdir()
                env = os.environ.copy()
                env.update(WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN='0',
                           WORMWOOD_WORKSTATION_GPU_TIMING='0')
                run('native-' + backend, [binary, '--check-bubble-notes', backend], 180, env)
                evidence = json.loads((destination / f'bubble-piano-roll-{backend}.json').read_text())
                assert evidence['entities_remaining'] == 0, evidence
                assert evidence['editor_error'] is None, evidence
                assert evidence['piano_roll']['grid_visible'] is False, evidence
                frame = json.loads((destination / 'workstation-optical-frame.json').read_text())
                assert frame['rows'][234][0] == 4, 'Native diagnostic did not submit four bubbles'
                ppm = destination / f'triangle-{backend}.ppm'
                magic, size, maximum, pixels = ppm.read_bytes().split(b'\n', 3)
                width, height = map(int, size.split())
                assert magic == b'P6' and maximum == b'255' and len(pixels) == width * height * 3
                assert max(pixels) > min(pixels), 'Empty/constant capture'
                run('preview-' + backend, ['/usr/bin/sips', '-s', 'format', 'png', ppm,
                    '--out', destination / 'bubble-piano-roll.png'], 30)
                report['native'][backend] = {'process': 'passed', 'width': width, 'height': height,
                                            'submitted_bubbles': 4, 'visual_review': 'pending'}
                save()
            native_binary.cleanup()
        report['status'] = 'checks_finished_visual_review_pending'
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        save()
        raise
    save()
    print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
