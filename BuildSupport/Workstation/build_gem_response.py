#!/usr/bin/env python3
"""Bake production transport-cap responses and validate them in the full scene.

Finite foreground payload for Codex Process Jobs. Reuses the validated Metal
probe; does not replace the installed application. Asset publication follows
successful Rust checks and paired full-scene GPU checks, not bake success alone.
"""
from pathlib import Path
import array
import datetime
import json
import math
import shutil
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
import gem_response as response
from check_compiled_gem import isolated_ruby, update_group
from types import SimpleNamespace

BASE = 3500544
RECORD = 16 + 128 * 128 * 60
ROWS = 4 * RECORD
REFERENCE = ROOT / 'output/checks/optics-rethink-20261031-061827-752714/inputs'
PROBE = ROOT / 'output/checks/gem-response-20261031-085359-797556/response-probe'
SOURCES = ['DecorativeControls/controls.bin', 'DecorativeControls/controls.json',
           'ControlEngraving/control_engraving.bin', 'ControlEngraving/control_engraving.json']


def fingerprint(data):
    h = 0xcbf29ce484222325
    for byte in data:
        h = ((h ^ byte) * 0x100000001b3) & 0xffffffffffffffff
    return f'{h:016x}'


def put(data, index, values):
    struct.pack_into('<4f', data, index * 16, *values)


def main():
    out = ROOT / 'output/checks' / ('gem-response-integration-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    print(f'Evidence: {out}', flush=True)
    report = {'status': 'running', 'installed': False, 'asset_published': False,
              'scope': 'four transport rubies; full-scene primary-ray integration', 'steps': []}

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    def run(name, args, timeout=120):
        print(name, flush=True)
        run_checked([str(v) for v in args], cwd=ROOT, log=out / (name + '.log'), timeout=timeout)
        report['steps'].append(name)
        save()

    try:
        if shutil.disk_usage(ROOT).free < 450 * 1024**2:
            raise RuntimeError('Need 450 MiB free for bounded bake and validation output')
        source = (REFERENCE / 'controls.bin').read_bytes()
        if len(source) != BASE * 16:
            raise ValueError('Optical response base no longer matches the fixture')
        controls = ROOT / 'Art/AudioWorkstation/DecorativeControls'
        manifest = json.loads((controls / 'controls.json').read_text())
        geometry = (controls / 'controls.bin').read_bytes()
        if source[manifest['base']*16:manifest['base']*16+len(geometry)] != geometry:
            raise ValueError('Captured fixture differs from the authored control geometry')
        source_hashes = {name: fingerprint((ROOT / 'Art/AudioWorkstation' / name).read_bytes()) for name in SOURCES}
        run('inspect-intent', [ROOT / 'output/macOS/MirrorPrismals.app/Contents/MacOS/wormwood-engine', '--inspect-intent'])
        run('generate-shaders', [sys.executable, 'Procedures/Shaders/generate_workstation.py'])
        run('validate-vulkan-shader', ['glslangValidator', '-V', 'Procedures/Shaders/workstation.frag', '-o', out/'workstation.spv'])
        run('validate-gl-shader', ['glslangValidator', 'Procedures/Shaders/workstation.gl.frag'])
        run('test-workstation', ['cargo', 'test', '--release', '--offline', '--locked', '--lib',
                                'workstation', '--', '--test-threads=1'], 1200)
        packed = out / 'response.bin'
        records = []
        with packed.open('wb') as stream:
            for instance in range(4):
                selected = bytearray(source)
                pose = response.row(source, 192 + instance*2)
                tree = response.row(source, 193 + instance*2)
                put(selected, 192, pose)
                put(selected, 193, tree)
                data, bounds, _ = isolated_ruby(selected)
                data = bytearray(data)
                # Bake each actual cap at its live size and light-relative pose.
                put(data, 192, pose)
                for index in (1, 2, 3, 4, 5):
                    put(data, index, response.row(source, index))
                update_group(data, bounds)
                training = out / 'training.bin'
                training.write_bytes(data)
                prefix = out / 'bake'
                response.prepare(SimpleNamespace(shader=REFERENCE / 'assets/Procedures/Shaders/workstation.wgsl',
                                                  frame=training, output=prefix, edge=128))
                result = out / 'baked'
                run(f'bake-{instance}', [PROBE, prefix.with_suffix('.wgsl'), prefix.with_suffix('.bin'),
                                        128, 128, result, 1, 8, 60])
                atlas = result.with_suffix('.rgba32f').read_bytes()
                if len(atlas) != 128*128*60*16:
                    raise ValueError('Truncated response bake')
                words = array.array('f')
                words.frombytes(atlas)
                if sys.byteorder != 'little': words.byteswap()
                if not all(math.isfinite(v) for v in words):
                    raise ValueError('Nonfinite response bake')
                meta = json.loads(prefix.with_suffix('.json').read_text())
                header = bytearray(16*16)
                put(header, 0, [731., 128., 60., float(instance)])
                put(header, 1, meta['bounds'])
                put(header, 2, pose)
                put(header, 3, tree)
                material = int(tree[3] if tree[3] > .5 else response.row(data, 12)[1]) + 40*instance
                for i in range(3): put(header, 4+i, response.row(data, material+i))
                for light in range(3):
                    lr = 16 + 5*light
                    delta = [response.row(data, lr)[i] - pose[i] for i in range(3)]
                    distance = math.sqrt(sum(v*v for v in delta))
                    gain = max(0., sum(response.row(data, lr+3)[i]*(-delta[i]/distance) for i in range(3))) / distance**2
                    put(header, 8+light, [max(gain, 1e-12), 0., 0., 0.])
                for i in range(3): put(header, 11+i, response.row(data, 214+i))
                put(header, 14, [response.row(data, 223)[3], 0., 0., 0.])
                put(header, 15, [meta['glyph_footprint_local'], 0., 0., 0.])
                stream.write(header)
                stream.write(atlas)
                records.append({'instance': instance, 'pose': pose, 'material': material,
                                'bounds': meta['bounds'], 'bake': json.loads(result.with_suffix('.json').read_text())})
                # These are this run's reusable scratch files, not earlier evidence.
                del words, atlas, data
        report['records'] = records
        atlas = packed.read_bytes()
        metadata = {'schema': 1, 'base': BASE, 'rows': ROWS, 'sources': source_hashes,
                    'fingerprint': fingerprint(atlas), 'records': records,
                    'limits': 'Fixed-view primary transport caps. Compatible paths are spatially filtered; discontinuities fall back. Moved-light engraving uses geometric gain. Exterior paths rejoin canonical scene tracing.'}
        (out / 'response.json').write_text(json.dumps(metadata, indent=2)+'\n')
        # Reuse one packet for both conditions. Baseline disables the first
        # descriptor only; the source shader and full scene are identical.
        packet = out / 'scene.bin'
        packet.write_bytes(source + atlas + b'\0' * (((BASE+ROWS+511)//512*512 - BASE-ROWS)*16))
        del atlas
        shader = ROOT / 'Procedures/Shaders/workstation.wgsl'
        shader_text = shader.read_text()
        # Diagnostic alpha records actual primary sample acceptance, rather than
        # mistaking an enabled descriptor or a faster fallback run for cache use.
        instrumented = shader_text.replace('responseUsed=true;finished=true;',
            'responseUsed=true;finished=true;responseUsedSamples+=1u;')
        instrumented = instrumented.replace('return vec4<f32>(gemDisplay(result/samples),1.0);',
            'return vec4<f32>(gemDisplay(result/samples),f32(responseUsedSamples));')
        if instrumented == shader_text or 'f32(responseUsedSamples)' not in instrumented:
            raise ValueError('Response instrumentation marker changed')
        instrumented = 'var<private> responseUsedSamples:u32=0u;\n' + instrumented
        (out / 'integrated.wgsl').write_text(instrumented)
        with packet.open('r+b') as stream:
            stream.seek(BASE*16); stream.write(struct.pack('<f', 0.))
        run('full-scene-baseline', [PROBE, out/'integrated.wgsl', packet, 640, 480, out/'baseline', 6, 8])
        with packet.open('r+b') as stream:
            stream.seek(BASE*16); stream.write(struct.pack('<f', 731.))
        run('full-scene-response', [PROBE, out/'integrated.wgsl', packet, 640, 480, out/'candidate', 6, 8])
        with packet.open('r+b') as stream:
            stream.seek(BASE*16); stream.write(struct.pack('<f', 0.))
        run('full-scene-baseline-repeat', [PROBE, out/'integrated.wgsl', packet, 640, 480, out/'baseline-repeat', 6, 8])
        with packet.open('r+b') as stream:
            stream.seek(BASE*16); stream.write(struct.pack('<f', 731.))
        report['baseline_repeat'] = json.loads((out/'baseline-repeat.json').read_text())
        for name in ('baseline', 'candidate'):
            report[name] = json.loads((out / (name+'.json')).read_text())
            run(name+'-png', ['/usr/bin/sips', '-s', 'format', 'png', out/(name+'.ppm'), '--out', out/(name+'.png')])
        pixels = array.array('f')
        pixels.frombytes((out / 'candidate.rgba32f').read_bytes())
        if sys.byteorder != 'little': pixels.byteswap()
        accepted = sum(pixels[3::4])
        report['accepted_primary_samples'] = accepted
        report['primary_samples'] = 640*480*2
        if accepted <= 0:
            raise ValueError('Full scene did not use any baked response samples')
        report['baseline_drift_percent'] = 100*(report['baseline_repeat']['median_gpu_ms']/report['baseline']['median_gpu_ms']-1)
        report['speedup_ratio'] = report['baseline']['median_gpu_ms'] / report['candidate']['median_gpu_ms']
        for name in SOURCES:
            if source_hashes[name] != fingerprint((ROOT / 'Art/AudioWorkstation' / name).read_bytes()):
                raise ValueError('Authored inputs changed during validation: ' + name)
        report['status'] = 'validated_pending_image_and_timing_review' 
        report['asset_staging'] = str(out)
        report['note'] = 'No installed app or production response asset replaced; review the full-scene result before publishing.'
        save()
        print(json.dumps({'status': report['status'], 'report': str(out/'report.json')}), flush=True)
    except BaseException as exc:
        report['status'] = 'failed'
        report['error'] = str(exc)
        save()
        raise


if __name__ == '__main__':
    main()
