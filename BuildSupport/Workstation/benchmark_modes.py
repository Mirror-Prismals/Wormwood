#!/usr/bin/env python3
"""Bounded native benchmarks: computer, VR, AR, game, and standalone DAW.

Build a fresh app with BuildSupport/macOS/build_app.py first. Runs are serial,
never install or mutate preferences, and preserve every failed attempt. FPS is
application loop rate, not scanout, headset refresh, or physical-input latency.
"""
from pathlib import Path
import argparse
import datetime
import json
import math
import os
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
sys.path.insert(0, str(Path(__file__).parent))
from profile_workstation_sustained import hardware, identity, environment_snapshot

MODES = ('computer', 'vr', 'ar', 'game', 'daw-only')
WORKLOADS = {
    'computer': 'Physical theatre computer from the spawn observer (about 4.7m from the monitor), real-time left-panel key replay; not a close-up computer view.',
    'vr': 'In-game VR pair, player view, forward/back navigation and look sweep plus real-time left-panel key replay.',
    'ar': 'In-game AR pair, frozen player observer plus real-time left-panel key replay.',
    'game': 'Normal planet_surface application, stationary spawn observer, active world simulation/rendering/audio; no DAW entry.',
    'daw-only': 'Standalone DAW, plain backdrop, real-time left-panel key replay; no synthetic MIDI/audio stress fixture.',
}


def read(path):
    return json.loads(Path(path).read_text())


def require(condition, message):
    if not condition:
        raise ValueError(message)


def stats(values):
    values = sorted(values)
    require(bool(values) and all(math.isfinite(v) and v > 0 for v in values), 'Missing/invalid loop intervals')
    q = lambda p: values[math.ceil(p * len(values)) - 1]
    return {'frames': len(values), 'seconds': sum(values) / 1000,
            'fps': 1000 / statistics.fmean(values), 'mean_ms': statistics.fmean(values),
            'p50_ms': q(.5), 'p95_ms': q(.95), 'p99_ms': q(.99), 'worst_ms': values[-1],
            'fraction_over_16_667_ms': sum(v > 1000 / 60 for v in values) / len(values)}


def counter(window, key):
    c = window.get(key, {})
    require(c.get('complete') is True and type(c.get('delta')) is int
            and c['delta'] >= 0 and c.get('end_lifetime_total', -1) - c.get('start_lifetime_total', -1) == c['delta'],
            'Incomplete measured audio counter: ' + key)
    return c['delta']


def summarize_daw(native, mode, seconds, warmup):
    embedded = mode != 'daw-only'
    require(native.get('entities_remaining') == 0, 'Application Entity cleanup incomplete')
    m = native.get('combined_workstation') if embedded else native
    require(isinstance(m, dict), 'Missing embedded workstation report')
    b = m['benchmark']
    require(b.get('complete') is True and b.get('warmup_frames') == warmup
            and b.get('fixed_simulation_step_seconds') is None
            and b.get('measured_loop_wall_seconds', 0) >= seconds
            and not b.get('resized_during_measurement'), 'Incomplete, fixed-step, short or resized replay')
    require(b.get('last_window', {}).get('device', {}).get('workstation_shader', {}).get('selected') == 'baked_images',
            'Expected production baked artwork')
    evidence = m.get('embedded_evidence')
    if embedded:
        require(evidence and evidence.get('embedded') is True and evidence.get('validated_frames', 0) >= b['measured_frames'],
                'Embedded observer not validated for every measured frame')
        require(evidence.get('computer_screen') == (mode == 'computer'), 'Wrong physical-screen mode')
        require(evidence.get('eyewear') == (mode if mode in ('ar', 'vr') else None), 'Wrong eyewear mode')
        require(evidence.get('observer_motion') == (mode == 'vr') and evidence.get('audio_rays') is False,
                'Wrong observer workload or diagnostic rays enabled')
        if mode == 'vr':
            poses = b.get('wearable_verification', [])
            require(len({tuple(p['player']) for p in poses if p.get('player')}) > 1, 'VR observer never moved')
    replay = b['motion_input']
    require(replay.get('gate') is False and replay.get('changes') is False, 'Unexpected fixed-step/content fixture')
    frames = replay['frames']
    moving = []
    for i in range(1, len(frames) - 1):
        require(frames[i+1]['frame'] == frames[i]['frame'] + 1, 'Missing drawn motion frame')
        if frames[i]['extensions'] != frames[i-1]['extensions']:
            moving.append((frames[i+1]['at'] - frames[i]['at']) * 1000)
    require(len(moving) >= 10 and len(replay.get('events', [])) >= 2, 'Panel input replay did not move panels')
    gpu = m.get('gpu_measurement', {})
    require(gpu.get('supported') is True and gpu.get('complete') is True, 'Incomplete GPU query coverage')
    window = m.get('benchmark_audio', {}).get('measured_counter_window')
    require(not embedded or isinstance(window, dict), 'Missing embedded audio counter window')
    return {'complete': True, 'dimensions': b['dimensions'], 'measured_frames': b['measured_frames'],
            'measured_seconds': b['measured_loop_wall_seconds'], 'whole_loop_fps': b['measured_loop_frames_per_second'],
            'whole_loop_timing': b['complete_loop_timing'], 'moving': stats(moving),
            'moving_scope': 'Exact panel-extension change versus previous record; current-to-next loop start, including reporting/pacing. Final interval excluded from motion subset.',
            'gpu': gpu, 'transport_underruns': counter(window, 'transport_underruns') if window else None,
            'vm_underrun_periods': counter(window, 'vm_underrun_periods') if window else None,
            'embedded_evidence': evidence, 'entry': m.get('entry'), 'device': b['device_last'].get('device')}


def summarize_game(native, timing, warmup, seconds):
    require(native.get('entities_remaining') == 0 and native.get('combined_workstation') is None,
            'Game entered DAW or failed cleanup')
    require(timing.get('dropped_frames') == 0, 'Game timing ring lost frames')
    frames = [f for f in timing['frames'] if f[0] > warmup and f[4]]
    selected = []
    elapsed = 0
    for frame in frames:
        require(frame[5] is True, 'Game measurement contains nondrawn frame')
        selected.append(frame)
        elapsed += frame[1] / 1000
        if elapsed >= seconds:
            break
    require(elapsed >= seconds, 'Game measurement too short; increase --game-frames')
    require(all(b[0] == a[0] + 1 for a, b in zip(selected, selected[1:])), 'Interrupted game measurement')
    result = stats([f[1] for f in selected])
    fresh = sum(i == 0 or f[6] != selected[i-1][6] for i, f in enumerate(selected))
    return {'complete': True, 'whole_loop_fps': result['fps'], 'whole_loop_timing': result,
            'measured_frames': len(selected), 'measured_seconds': elapsed,
            'fresh_scene_fps': fresh / elapsed,
            'pump_mean_ms': statistics.fmean(f[2] for f in selected),
            'update_mean_ms': statistics.fmean(f[3] for f in selected),
            'audio_lifetime_including_startup_and_cleanup': native.get('environment_audio'),
            'audio_limit': 'Game-only report has no measured-window counter snapshots; lifetime underruns must not be compared with DAW measured deltas.'}


def command_env(mode, args, dest, assets):
    # Native audio/plugin setup may change the process working directory.
    dest, assets = dest.resolve(), assets.resolve()
    env = {k: v for k, v in os.environ.items() if not k.startswith('WORMWOOD_')}
    env.update(WORMWOOD_ASSET_ROOT=str(assets), WORMWOOD_OUTPUT_ROOT=str(dest),
               WORMWOOD_SETTINGS_PATH=str(dest / 'settings.json'), WORMWOOD_FULLSCREEN='1',
               WORMWOOD_WORKSTATION_RETINA='0', WORMWOOD_PERFORMANCE='1', WORMWOOD_GAME_FRAME_TIMING='1')
    if mode == 'game':
        return ['--interactive', 'planet_surface', 'WebGPU', '--close-after-frames', str(args.game_frames)], env
    env.update(WORMWOOD_WORKSTATION_BENCHMARK_WARMUP=str(args.warmup),
               WORMWOOD_WORKSTATION_BENCHMARK_CYCLE='32', WORMWOOD_WORKSTATION_BENCHMARK_MIN_SECONDS=str(args.seconds),
               WORMWOOD_WORKSTATION_BENCHMARK_PACED='1', WORMWOOD_WORKSTATION_MOTION_INPUT_TEST='motion')
    if mode == 'computer':
        env['WORMWOOD_COMPUTER_SCREEN_VERIFY'] = '1'
    elif mode in ('ar', 'vr'):
        env['WORMWOOD_WORKSTATION_BENCHMARK_EYEWEAR'] = mode
        if mode == 'vr':
            env['WORMWOOD_WORKSTATION_BENCHMARK_OBSERVER_MOTION'] = '1'
    flag = '--benchmark-workstation' if mode == 'daw-only' else '--benchmark-game-workstation'
    return [flag, 'WebGPU', '--frames', '600'], env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, required=True)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--mode', choices=MODES, action='append')
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--seconds', type=float, default=30)
    parser.add_argument('--warmup', type=int, default=120)
    parser.add_argument('--game-frames', type=int, default=2400)
    args = parser.parse_args()
    if not (1 <= args.repeats <= 5 and 1 <= args.warmup <= 120 and 1 <= args.game_frames <= 3600
            and math.isfinite(args.seconds) and 1 <= args.seconds <= 120):
        parser.error('repeats1..5, warmup1..120, game-frames1..3600, seconds1..120 required')
    modes = args.mode or list(MODES)
    app = args.app.resolve()
    binary = app / 'Contents/MacOS/wormwood-engine'
    assets = app / 'Contents/Resources/EngineAssets'
    out = args.output or ROOT / 'output/benchmarks' / ('five-modes-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S'))
    out = out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    build = identity(app)
    report = {'schema': 1, 'status': 'running', 'build': build, 'hardware': hardware(),
              'workloads': {m: WORKLOADS[m] for m in modes}, 'request': vars(args) | {'app': str(app), 'output': str(out)},
              'limits': ['Empty default document; no plugin, recording, loaded-project or synthetic MIDI stress.',
                         'Game uses a stationary spawn observer; not a traversal stress test.',
                         'Computer screen is perspective-mapped; AR/VR are the game wearable modes, not headset hardware or stereoscopic rendering.',
                         'FPS is complete application loop rate, not physical scanout or input latency.',
                         'Separate startup/warmup excluded; no thermal steady-state or exclusive hardware claim.',
                         'Game window uses first requested duration after warmup; remaining bounded game frames retained in raw report.',
                         'Moving subset is left-panel open/close/reversal only; no manual drag or other-panel claim.'],
              'runs': []}
    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2, default=str) + '\n')
    save()
    settings_path = Path.home() / 'Library/Application Support/MirrorPrismals/workstation-settings.json'
    settings = read(settings_path) if settings_path.is_file() else None
    if settings is not None:
        report['initial_settings'] = settings
    # Rotate mode order across repeats to distribute warm-device/order effects.
    for repeat in range(args.repeats):
        order = modes[repeat % len(modes):] + modes[:repeat % len(modes)]
        for mode in order:
            dest = out / f'{repeat+1}-{mode}'
            dest.mkdir()
            if settings is not None:
                (dest / 'settings.json').write_text(json.dumps(settings))
            argv, env = command_env(mode, args, dest, assets)
            run = {'mode': mode, 'repeat': repeat+1, 'output': str(dest), 'command': [str(binary), *argv],
                   'environment': {k: v for k, v in env.items() if k.startswith('WORMWOOD_')}, 'before': environment_snapshot()}
            report['runs'].append(run)
            save()
            print(dest.name, flush=True)
            start = time.monotonic()
            try:
                run_checked(run['command'], cwd=ROOT, env=env, log=dest / 'native.log', timeout=300)
                if mode == 'game':
                    run['measurement'] = summarize_game(read(dest / 'interactive-WebGPU.json'), read(dest / 'game-frame-timing.json'), args.warmup, args.seconds)
                    gpu = read(dest / 'performance-gpu-WebGPU-GameGraphics.json')
                    require(gpu.get('supported') is True and gpu.get('incomplete_samples') == 0
                            and gpu.get('submitted_frames') == gpu.get('completed_samples')
                            and not gpu.get('samples_truncated'), 'Incomplete game GPU query coverage')
                    run['measurement']['gpu_coverage'] = {k: gpu.get(k) for k in
                        ('scope', 'method', 'submitted_frames', 'completed_samples', 'incomplete_samples')}
                else:
                    name = 'workstation-benchmark-WebGPU.json' if mode == 'daw-only' else 'game-workstation-benchmark-WebGPU.json'
                    run['measurement'] = summarize_daw(read(dest / name), mode, args.seconds, args.warmup)
                run['status'] = 'complete'
                print(json.dumps({'mode': mode, 'fps': run['measurement']['whole_loop_fps'], 'moving': run['measurement'].get('moving', {}).get('fps')}), flush=True)
            except Exception as error:
                run.update(status='failed', error=str(error))
                print(f'{mode}: FAILED: {error}', flush=True)
            run.update(process_seconds=time.monotonic()-start, after=environment_snapshot())
            save()
    report['final_build'] = identity(app)
    report['build_unchanged'] = report['final_build'] == build
    report['status'] = 'complete' if report['build_unchanged'] and all(r['status'] == 'complete' for r in report['runs']) else 'incomplete'
    report['summary'] = {}
    lines = ['# Five-mode native benchmark', '', f"Status: {report['status']}", '', '| Mode | Loop FPS range | Moving FPS range | Moving p99 ms range |', '|---|---:|---:|---:|']
    for mode in modes:
        runs = [r['measurement'] for r in report['runs'] if r['mode'] == mode and r['status'] == 'complete']
        if not runs:
            lines.append(f'| {mode} | FAILED | — | — |')
            continue
        fps = [r['whole_loop_fps'] for r in runs]
        moving = [r['moving']['fps'] for r in runs if 'moving' in r]
        p99 = [r['moving']['p99_ms'] for r in runs if 'moving' in r]
        report['summary'][mode] = {'complete_runs': len(runs), 'loop_fps_range': [min(fps), max(fps)],
                                   'moving_fps_range': [min(moving), max(moving)] if moving else None,
                                   'moving_p99_ms_range': [min(p99), max(p99)] if p99 else None}
        show = lambda x: f'{min(x):.1f}–{max(x):.1f}' if x else '—'
        lines.append(f'| {mode} | {show(fps)} | {show(moving)} | {show(p99)} |')
    lines += ['', *[f'- {x}' for x in report['limits']], '']
    (out / 'summary.md').write_text('\n'.join(lines))
    save()
    print(json.dumps({'status': report['status'], 'report': str(out / 'report.json')}), flush=True)
    return 0 if report['status'] == 'complete' else 1


if __name__ == '__main__':
    raise SystemExit(main())
