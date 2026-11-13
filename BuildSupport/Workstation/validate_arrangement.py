"""Finite source/build/intent validation and optional native arrangement capture.

Run through Codex Process Jobs for the full build. No app install, packaging,
publishing, microphone capture or live audio is requested by this diagnostic.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import struct
import subprocess
import sys
import time
import zlib

ROOT = Path(__file__).resolve().parents[2]


def source_hashes():
    paths = list((ROOT / 'Workstation').glob('*.rs'))
    paths += list((ROOT / 'Workstation').glob('*.glsl'))
    paths += [ROOT / p for p in (
        'BaseSystem/LayerFive/AudioWorkstationSystem.rs',
        'BaseSystem/LayerFour/WorkstationPanelSystem.rs',
        'Render/Workstation.rs', 'Procedures/Shaders/workstation.shared.glsl',
        'Procedures/Shaders/workstation.wgsl', 'Procedures/Shaders/workstation.gl.frag',
        'Systems/AudioWorkstationSystem.json', 'Systems/WorkstationPanelSystem.json')]
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in paths}


def png_from_ppm(path, output):
    magic, size, maximum, pixels = path.read_bytes().split(b'\n', 3)
    width, height = map(int, size.split())
    assert magic == b'P6' and maximum == b'255' and len(pixels) == width * height * 3
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    scanlines = b''.join(b'\0' + pixels[y*width*3:(y+1)*width*3] for y in range(height))
    output.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
                       + chunk(b'IDAT', zlib.compress(scanlines)) + chunk(b'IEND', b''))
    return [width, height]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--native', choices=['WebGPU', 'Vulkan', 'OpenGL'])
    args = parser.parse_args()
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = ROOT / 'output/checks' / ('genma-arrangement-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    report = {'status': 'running', 'checks': [], 'evidence': str(out),
              'visual_review': 'pending', 'native_audio': 'not_exercised'}
    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    def run(name, argv, timeout=1200, env=None):
        print(name, flush=True)
        start = time.monotonic()
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(list(map(str, argv)), cwd=ROOT, env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
        report['checks'].append({'name': name, 'exit_code': result.returncode,
                                 'seconds': round(time.monotonic() - start, 3)})
        save()
        if result.returncode:
            print((out / (name + '.log')).read_text(errors='replace')[-16000:])
            raise RuntimeError(f'{name} failed with exit {result.returncode}')
    try:
        run('shader-generation', [sys.executable, 'Procedures/Shaders/generate_workstation.py'], 30)
        initial = source_hashes()
        (out / 'source-hashes-before.json').write_text(json.dumps(initial, indent=2) + '\n')
        for frontend in ('gl', 'ios'):
            run('shader-' + frontend, ['glslangValidator', '-S', 'frag',
                f'Procedures/Shaders/workstation.{frontend}.frag'], 120)
        run('shader-vulkan', ['glslangValidator', '-V', 'Procedures/Shaders/workstation.frag',
            '-o', out / 'workstation.spv'], 120)
        run('workstation-tests', ['cargo', 'test', '--offline', '--locked', 'workstation',
            '--', '--test-threads=1'])
        run('desktop-build', ['cargo', 'build', '--offline', '--locked'])
        binary = ROOT / 'target/debug/wormwood'
        run('intent', [binary, '--inspect-intent', '--json'], 30)
        intent = json.loads((out / 'intent.log').read_text())
        report['intent_summary'] = intent['summary']
        assert intent['summary']['active_structural_mismatches'] == 0, 'intent mismatch'
        report['status'] = 'source_checks_passed'
        save()
        if args.native:
            backend = args.native
            destination = out / backend
            destination.mkdir()
            env = dict(os.environ, WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN='0',
                       WORMWOOD_WORKSTATION_GPU_TIMING='0')
            run('native-' + backend, [binary, '--check-arrangement', backend], 240, env)
            scene = json.loads((destination / f'arrangement-{backend}.json').read_text())
            assert scene['entities_remaining'] == 0 and scene['arrangement_error'] is None
            frame = json.loads((destination / 'workstation-optical-frame.json').read_text())
            assert frame['rows'][248][0] >= 2, 'timeline instances missing'
            report['capture'] = str(destination / 'arrangement.png')
            report['capture_size'] = png_from_ppm(destination / f'triangle-{backend}.ppm',
                                                  destination / 'arrangement.png')
            report['status'] = 'source_and_native_capture_passed_visual_review_pending'
        final = source_hashes()
        (out / 'source-hashes-after.json').write_text(json.dumps(final, indent=2) + '\n')
        report['concurrent_changes'] = sorted(p for p in initial.keys() | final.keys()
                                              if initial.get(p) != final.get(p))
        if report['concurrent_changes']:
            report['status'] = 'checks_finished_sources_changed_review_required'
        save()
        print(json.dumps(report, indent=2), flush=True)
    except Exception as error:
        report['status'] = 'failed'
        report['error'] = str(error)
        save()
        print(json.dumps(report, indent=2), flush=True)
        raise


if __name__ == '__main__':
    main()
