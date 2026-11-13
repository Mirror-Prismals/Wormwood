#!/usr/bin/env python3
"""Bounded native-density comparison; leave the packaged app unchanged."""
import datetime
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'output/checks' / ('engraving-density-' + datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(parents=True)
report = {'output': str(OUT), 'steps': [], 'packaged': False}

def run(name, argv, timeout, env=None):
    print(name, flush=True)
    started = time.monotonic()
    with (OUT / (name + '.log')).open('w') as log:
        try:
            code = subprocess.run([str(x) for x in argv], cwd=ROOT, env=env,
                                  stdout=log, stderr=subprocess.STDOUT,
                                  timeout=timeout).returncode
        except subprocess.TimeoutExpired:
            code = 124
    report['steps'].append({'name': name, 'status': code,
                            'seconds': round(time.monotonic()-started, 2)})
    (OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    if code:
        print((OUT / (name + '.log')).read_text(errors='replace')[-5000:], flush=True)
        raise SystemExit(code)

run('native-build', ['cargo', 'build', '--offline', '--locked', '--release', '--bin', 'wormwood'], 300)
for retina in ['0', '1']:
    dest = OUT / ('Retina-' + retina)
    dest.mkdir()
    env = os.environ.copy()
    env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(dest),
               WORMWOOD_FULLSCREEN='0', WORMWOOD_WORKSTATION_RETINA=retina)
    run('render-' + retina,
        [ROOT / 'target/release/wormwood', '--workstation', 'WebGPU', '--frames', '36'], 90, env)
    for p in dest.glob('*.ppm'):
        subprocess.run(['sips', '-s', 'format', 'png', str(p), '--out', str(p.with_suffix('.png'))],
                       check=True, stdout=subprocess.DEVNULL)
    events = [json.loads(line) for line in (dest / 'execution.jsonl').read_text().splitlines()]
    durations = [e['elapsed_us']/1000 for e in events
                 if e.get('event') == 'finished' and e.get('function') == 'GraphicsConnectionSystem.Pump'][6:]
    optical = json.loads((dest / 'workstation-optical-frame.json').read_text())
    report['retina_' + retina] = {
        'framebuffer': optical['rows'][8][:2],
        'logical_viewport': optical['logical_viewport'],
        'pump_ms_median': statistics.median(durations),
        'pump_ms_max': max(durations),
        'measured_frames': len(durations),
    }
(OUT / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report), flush=True)
