#!/usr/bin/env python3
"""Finite repeated native graphics measurements; never builds, installs or uploads.

Run through Codex Process Jobs. Default replay needs the opt-in benchmark
controls introduced with this harness. --legacy permits honest measurements of
older installed apps, whose single motion sequence is not sustained motion.
Unsupported requests remain explicit coverage gaps, never inferred successes.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import math
import os
import platform
import plistlib
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked

VARIANT_FLAGS = frozenset({'WORMWOOD_WORKSTATION_PIXEL_TRANSPORT',
                           'WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE',
                           'WORMWOOD_WORKSTATION_RESOLVE_REUSE'})
RECOVERABLE_DRAW_REASONS = frozenset({'iconified', 'zero_framebuffer', 'zero_logical_dimensions'})


def parse_variant_flags(values):
    result = {}
    for value in values:
        key, separator, setting = value.partition('=')
        if not separator or key not in VARIANT_FLAGS or setting not in ('0', '1'):
            raise ValueError('Variant flag must be an allowlisted KEY=0 or KEY=1: ' + value)
        if key in result:
            raise ValueError('Duplicate variant flag: ' + key)
        result[key] = setting
    return result


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read_json(path, fallback=None):
    return json.loads(path.read_text()) if path.is_file() else fallback


def object_or_empty(value):
    return value if isinstance(value, dict) else {}


def valid_counter_delta(value):
    value = object_or_empty(value)
    counts = [value.get(key) for key in ('start_lifetime_total', 'end_lifetime_total', 'delta')]
    return (value.get('complete') is True and all(type(count) is int and count >= 0 for count in counts)
            and counts[1] >= counts[0] and counts[2] == counts[1] - counts[0])


def metrics(values):
    values = sorted(float(value) for value in values if value is not None)
    if not values:
        return None
    if any(not math.isfinite(value) or value < 0 for value in values):
        raise ValueError('Nonfinite or negative timing sample')
    def percentile(fraction):
        # Nearest-rank: p99 of 600 samples is sample594, not an interpolation.
        return values[max(0, math.ceil(fraction * len(values)) - 1)]
    mean = statistics.fmean(values)
    return {'count': len(values), 'mean_ms': mean, 'p50_ms': percentile(.5),
            'p95_ms': percentile(.95), 'p99_ms': percentile(.99),
            'worst_ms': values[-1], 'cost_equivalent_fps': 1000 / mean if mean else None,
            'fraction_over_16_667_ms': sum(v > 1000 / 60 for v in values) / len(values)}


def valid_interval_ms(value):
    return (isinstance(value, (int, float)) and not isinstance(value, bool)
            and math.isfinite(value) and value >= 0)


def interval_metrics(values):
    """Nonexclusive elapsed intervals do not have an independent FPS cost."""
    result = metrics(values)
    if result is not None:
        result.pop('cost_equivalent_fps', None)
    return result


def capture(command, timeout=20):
    try:
        result = subprocess.run(command, text=True, capture_output=True, timeout=timeout)
        return {'command': command, 'exit_code': result.returncode,
                'stdout': result.stdout[:131072], 'stderr': result.stderr[:4096]}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {'command': command, 'available': False, 'error': str(error)}


def hardware():
    raw = capture(['/usr/sbin/system_profiler', '-json', 'SPHardwareDataType',
                   'SPDisplaysDataType'], 45)
    selected = None
    if raw.get('exit_code') == 0:
        try:
            value = json.loads(raw['stdout'])
            # Hardware UUID and serial numbers have no measurement value.
            def sanitize(item):
                if isinstance(item, dict):
                    return {k: sanitize(v) for k, v in item.items()
                            if not any(word in k.lower() for word in ('serial', 'uuid', 'udid'))}
                return [sanitize(v) for v in item] if isinstance(item, list) else item
            selected = sanitize(value)
            raw.pop('stdout')
        except ValueError:
            raw['stdout'] = None
    return {'platform': platform.platform(), 'machine': platform.machine(),
            'system_profiler': selected, 'probe': raw,
            'selected_window_monitor': None,
            'refresh_note': 'Display modes are inventory; selected-window refresh is not instrumented.'}


def environment_snapshot():
    return {'utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
            'power': capture(['/usr/bin/pmset', '-g', 'batt']),
            'thermal': capture(['/usr/bin/pmset', '-g', 'therm']),
            'cpu_contenders': capture(['/bin/ps', '-axo', 'pid=,pcpu=,comm=']),
            'conditions': 'Observed snapshots only; no power settings changed or other processes stopped. '
                          'Temperature, GPU frequency and exclusive hardware use are not inferred.'}


def identity(app):
    app = app.resolve()
    executable = app / 'Contents/MacOS/wormwood-engine'
    if not executable.is_file():
        raise ValueError(f'Packaged engine missing: {executable}')
    assets = app / 'Contents/Resources/EngineAssets'
    paths = sorted(p for subtree in ('Art', 'Procedures/Shaders')
                   for p in (assets / subtree).rglob('*') if p.is_file())
    authored = {str(p.relative_to(assets)): digest(p) for p in paths}
    info_path = app / 'Contents/Info.plist'
    info = plistlib.loads(info_path.read_bytes()) if info_path.is_file() else {}
    return {'app': str(app), 'executable_sha256': digest(executable),
            'bundle_version': info.get('CFBundleVersion'),
            'short_version': info.get('CFBundleShortVersionString'),
            'art_and_shader_hashes': authored}


def trace_summary(path):
    wanted = {'AudioWorkstationSystem.Step', 'WorkstationPanelSystem.View',
              'GraphicsConnectionSystem.SetView', 'GraphicsConnectionSystem.Pump',
              'EnvironmentAudioSystem.Pump'}
    values = {name: [] for name in wanted}
    if path.is_file():
        with path.open() as source:
            for line in source:
                row = json.loads(line)
                if row.get('event') == 'finished' and row.get('function') in wanted:
                    value = row.get('elapsed_us')
                    if value is not None:
                        values[row['function']].append(value / 1000)
    return {'scope': 'Inclusive named-hook wall durations across the whole session, including startup/warmup. '
                     'Nested durations overlap; these are not thread CPU time or additive stages.',
            'hooks': {name: metrics(samples) for name, samples in values.items()}}


def summarize(destination, legacy, warmup, cycle, requested, audio_requested, scenario, minimum_seconds):
    native = read_json(destination / 'workstation-benchmark-WebGPU.json', {})
    benchmark = native.get('benchmark', {})
    gpu = read_json(destination / 'performance-gpu-WebGPU.json', {})
    performance = read_json(destination / 'performance.json', {})
    selection = read_json(destination / 'workstation-shader-WebGPU.json', {})
    samples = benchmark.get('samples', [])
    actual_warmup = benchmark.get('warmup_frames')
    elapsed = benchmark.get('measured_loop_wall_seconds')
    count_matched = (len(samples) >= requested and len(samples) <= 20_000
                     if minimum_seconds > 0 else len(samples) == requested)
    duration_matched = (minimum_seconds == 0 or (isinstance(elapsed, (int, float))
                        and math.isfinite(elapsed) and elapsed >= minimum_seconds
                        and benchmark.get('minimum_measured_seconds') == minimum_seconds
                        and benchmark.get('maximum_measured_frames') == 20_000
                        and benchmark.get('requested_measured_frames') == requested))
    request_matched = (benchmark.get('complete') is True and count_matched and duration_matched
                       and actual_warmup == (4 if legacy else warmup)
                       and (legacy or (benchmark.get('benchmark_controls_version', 0) >= (2 if minimum_seconds else 1)
                            and benchmark.get('cycle_frames') == (None if scenario == 'idle' else cycle)
                            and benchmark.get('scenario') == ('workstation-settled-idle-v1' if scenario == 'idle'
                                else 'origami-panel-motion-repeated-v1')))
                       and (not audio_requested or benchmark.get('audio_playback_requested') is True))
    gpu_by_draw = {sample['draw']: sample['gpu_ms'] for sample in gpu.get('samples', [])}
    query_by_draw = {sample['draw']: sample.get('raw', {})
                     for sample in gpu.get('query_observations', [])}
    groups = {'all_measured': samples}
    for phase in range(1, 5):
        groups[f'phase_{phase}'] = [sample for sample in samples if sample.get('phase') == phase]
    if query_by_draw:
        groups['cache_rebuilt'] = [sample for sample in samples
            if any(query_by_draw.get(sample['draw_index'], {}).get('active_passes', [])[:2])]
        groups['cache_reused'] = [sample for sample in samples
            if sample['draw_index'] in query_by_draw and not any(
                query_by_draw[sample['draw_index']].get('active_passes', [])[:2])]
    result = {}
    for name, group in groups.items():
        result[name] = {key: metrics(sample.get(key) for sample in group)
                        for key in ('frame_ms', 'update_ms', 'render_present_ms')}
        actual = [gpu_by_draw[sample['draw_index']] for sample in group
                  if sample['draw_index'] in gpu_by_draw]
        result[name]['gpu_ms'] = metrics(actual)
        result[name]['gpu_complete'] = gpu.get('supported') is True and len(actual) == len(group)
        passes = {}
        for index, label in enumerate(('panel_bake', 'general_bake', 'lighting_resolve', 'optical_render_pass')):
            observed, amortized = [], []
            missing_draws = []
            for sample in group:
                raw = query_by_draw.get(sample['draw_index'], {})
                durations = raw.get('per_pass_ms', [])
                active = raw.get('active_passes', [])
                if len(durations) == 4 and len(active) == 4:
                    if active[index] is True and valid_interval_ms(durations[index]):
                        observed.append(durations[index])
                        amortized.append(durations[index])
                    elif active[index] is False:
                        amortized.append(0.)
                    else:
                        missing_draws.append(sample['draw_index'])
                else:
                    missing_draws.append(sample['draw_index'])
            passes[label] = {'active_samples': interval_metrics(observed),
                             'amortized_over_observed_frames': interval_metrics(amortized),
                             'expected_frames': len(group),
                             'observed_frames': len(amortized),
                             'complete': len(amortized) == len(group),
                             'missing_or_invalid_draws': missing_draws,
                             'scope': 'Nonexclusive raw pass-boundary elapsed interval; cannot be summed as shader cost.'}
        result[name]['gpu_passes'] = passes if query_by_draw else None
        result[name]['gpu_passes_complete'] = bool(query_by_draw) and all(
            value['complete'] for value in passes.values())
        for field in ('active_pass_envelope_ms', 'interval_union_ms', 'interval_overlap_ms',
                      'interval_uncovered_ms', 'legacy_first_active_to_render_end_ms'):
            result[name]['gpu_' + field] = interval_metrics(
                query_by_draw.get(sample['draw_index'], {}).get(field) for sample in group)
    warmup_samples = benchmark.get('warmup_samples', [])
    audio = performance.get('audio', {})
    playback = native.get('benchmark_audio', {})
    native_audio = playback.get('cleanup', {}).get('native', {}) if isinstance(playback.get('cleanup'), dict) else {}
    transport = playback.get('final_status', {}).get('transport', {})
    if not isinstance(transport, dict):
        transport = {}
    audio_observed = (playback.get('started') is True and audio.get('observed_callbacks', 0) > 0
        and native_audio.get('samples_produced', 0) > 0 and native_audio.get('stream_output_peak', 0) > 0
        and transport.get('position_frame', 0) > 0 and not playback.get('runtime_error'))
    return {'request_matched': request_matched, 'native_complete': benchmark.get('complete'),
            'native_end_reason': native.get('end_reason', benchmark.get('end_reason')),
            'native_errors': {key: native[key] for key in ('error', 'gpu_error') if native.get(key)},
            'benchmark_controls_version': benchmark.get('benchmark_controls_version'),
            'last_window': benchmark.get('last_window'),
            'resized_during_measurement': benchmark.get('resized_during_measurement'),
            'scenario': benchmark.get('scenario'), 'warmup_frames': actual_warmup,
            'cycle_frames': benchmark.get('cycle_frames'), 'measured_frames': len(samples),
            'minimum_requested_frames': requested, 'minimum_measured_seconds': minimum_seconds,
            'maximum_measured_frames': benchmark.get('maximum_measured_frames', requested),
            'measured_loop_wall_seconds': benchmark.get('measured_loop_wall_seconds'),
            'measured_loop_fps': benchmark.get('measured_loop_frames_per_second'),
            'timed_update_render_sections_seconds': sum(s['frame_ms'] for s in samples) / 1000.,
            'duration_semantics': 'measured_loop_wall_seconds is actual elapsed measurement time. '
                'Timed update/render sections exclude loop bookkeeping. Frame count is not a duration guarantee.',
            'framebuffer_dimensions': benchmark.get('dimensions'),
            'logical_dimensions_observed': sorted({tuple(s['logical_dimensions']) for s in samples
                if 'logical_dimensions' in s}, key=str),
            'device_first': benchmark.get('device_first'), 'device_last': benchmark.get('device_last'),
            'groups': result, 'warmup_wall_ms': metrics(s.get('frame_ms') for s in warmup_samples),
            'first_draw': warmup_samples[0] if warmup_samples else None,
            'gpu_warmup_ms': metrics(value for draw, value in gpu_by_draw.items()
                                     if actual_warmup is not None and draw <= actual_warmup),
            'gpu_scope': gpu.get('scope'), 'gpu_method': gpu.get('method'),
            'gpu_duration_semantics': (gpu.get('query_context') or {}).get(
                'gpu_duration_semantics', 'legacy_first_active_to_render_end_v1'),
            'gpu_pass_interval_semantics': 'Nonexclusive, potentially overlapping backend intervals; render interval is not fragment-shader-only cost.',
            'gpu_supported': gpu.get('supported'), 'gpu_pass_breakdown_observed': bool(query_by_draw),
            'gpu_measurement_complete': result['all_measured']['gpu_complete'],
            'cpu_stage_wall_timing': read_json(destination / 'workstation-cpu-stages.json'),
            'cpu_thread_telemetry': performance.get('categories'),
            'cpu_telemetry_semantics': performance.get('semantics'),
            'shader_selection': selection, 'named_hooks': trace_summary(destination / 'execution.jsonl'),
            'audio': {'callback_telemetry': audio,
                      'actual_callbacks_observed': audio.get('observed_callbacks', 0) > 0,
                      'playback_requested': audio_requested,
                      'actual_audio_playback_observed': audio_observed,
                      'playback_evidence': playback,
                      'measured_counter_window': playback.get('measured_counter_window'),
                      'blocker': None if audio_observed else (playback.get('unavailable_reason')
                          or playback.get('runtime_error') or 'Actual nonzero native playback not established by this run.'),
                      'native_device_and_callback_info': native_audio or None,
                      'meters': 'Static authored geometry; actual audio-driven meter animation is not exercised.',
                      'output_audibility_confirmed': False,
                      'startup_jack_diagnostic': read_json(destination / 'jack-diagnostic.json'),
                      'event_to_effect_latency_measured': False}}


def incomplete_reason(run):
    """Explain evidence from this attempt; never recommend hiding a failure with legacy mode."""
    measurement = run.get('measurement', {})
    errors = measurement.get('native_errors')
    if errors:
        return 'Native failure: ' + json.dumps(errors, sort_keys=True)
    if run.get('exit_success') is not True and (run.get('process_failure_kind') != 'exit_status'
                                              or run.get('exit_code') != 1):
        return ('Native process failed (' + str(run.get('process_failure_kind', 'unknown'))
                + '): ' + str(run.get('error', 'no native completion report')))
    reason = measurement.get('native_end_reason')
    window = object_or_empty(measurement.get('last_window'))
    status = object_or_empty(window.get('draw_status'))
    if reason == 'benchmark_nondrawable':
        return ('Native window could not draw: ' + str(status.get('reason', 'unknown'))
                + '; draw_status=' + json.dumps(status, sort_keys=True))
    if reason and reason not in ('frame_limit', 'benchmark_duration_complete'):
        return 'Native replay ended with ' + str(reason)
    if run.get('exit_success') is not True:
        return ('Native process failed (' + str(run.get('process_failure_kind', 'unknown'))
                + '): ' + str(run.get('error', 'no native completion report')))
    if measurement.get('resized_during_measurement'):
        return 'Observed framebuffer dimensions changed during measurement'
    return ('Requested replay was not completed: controls_version='
            + str(measurement.get('benchmark_controls_version'))
            + ', measured_frames=' + str(measurement.get('measured_frames'))
            + ', measured_seconds=' + str(measurement.get('measured_loop_wall_seconds'))
            + ', native_complete=' + str(measurement.get('native_complete')))


def attempt_disposition(run, attempt_number, maximum_attempts):
    """Only explicit pre-draw window interruptions may restart a whole session."""
    if run.get('exit_success') is True and run.get('measurement', {}).get('request_matched') is True:
        return {'action': 'include', 'reason': 'Complete uninterrupted native attempt'}
    measurement = run.get('measurement', {})
    window = object_or_empty(measurement.get('last_window'))
    status = object_or_empty(window.get('draw_status'))
    audio = object_or_empty(measurement.get('audio', {}).get('playback_evidence'))
    ordinary_exit = (run.get('exit_success') is True or
        (run.get('process_failure_kind') == 'exit_status' and run.get('exit_code') == 1))
    recoverable = (ordinary_exit and measurement.get('native_complete') is False
        and measurement.get('native_end_reason') == 'benchmark_nondrawable'
        and not measurement.get('native_errors') and not audio.get('runtime_error')
        and not object_or_empty(audio.get('final_status')).get('error')
        and window.get('drawn') is False and window.get('closed') is False
        and isinstance(status.get('reason'), str) and status['reason'] in RECOVERABLE_DRAW_REASONS
        and status.get('recoverable') is True and status.get('attempted') is False)
    reason = incomplete_reason(run)
    if recoverable and attempt_number < maximum_attempts:
        return {'action': 'retry_fresh_session', 'reason': reason,
                'samples_discarded_from_comparison': True}
    return {'action': 'fail', 'reason': reason, 'recoverable_condition': recoverable,
            'attempt_limit_reached': recoverable and attempt_number >= maximum_attempts,
            'samples_discarded_from_comparison': True}


def comparison_signature(run):
    """Observed workload/settings, excluding only the intended variant flags."""
    measurement = run['measurement']
    signature = {key: measurement.get(key) for key in
        ('scenario', 'warmup_frames', 'cycle_frames', 'minimum_requested_frames',
         'minimum_measured_seconds', 'maximum_measured_frames',
         'framebuffer_dimensions', 'logical_dimensions_observed', 'gpu_method', 'gpu_supported',
         'gpu_duration_semantics')}
    # Faster duration-bounded variants render more frames by design. They still
    # require the same minimums and complete cycles, rather than identical counts.
    if not measurement.get('minimum_measured_seconds'):
        signature['measured_frames'] = measurement.get('measured_frames')
    signature['environment'] = {key: value for key, value in run.get('effective_environment', {}).items()
        if key not in VARIANT_FLAGS and key not in ('WORMWOOD_ASSET_ROOT', 'WORMWOOD_OUTPUT_ROOT')}
    device = measurement.get('device_last') or {}
    signature['graphics_device'] = {key: device.get(key) for key in
        ('api', 'implementation', 'native_backend', 'device', 'description', 'width', 'height')}
    audio = measurement.get('audio', {})
    signature['audio_playback_requested'] = audio.get('playback_requested')
    signature['actual_audio_playback_observed'] = audio.get('actual_audio_playback_observed')
    window = object_or_empty(audio.get('measured_counter_window'))
    signature['audio_counter_window_version'] = window.get('version')
    signature['audio_counter_boundary_mode'] = window.get('boundary_mode')
    native = audio.get('native_device_and_callback_info') or {}
    signature['native_audio_configuration'] = {key: native.get(key) for key in
        ('output_device_uid', 'sample_rate_hz', 'buffer_frames', 'buffer_budget_ms', 'mode', 'realtime')}
    return signature


def repeat_comparison(runs, variants, expected_repeats):
    """Fail the whole aggregate closed when ANY included run is unmatched.

    Every individual measurement remains saved. We do not silently select the
    first or fastest matching pair and pool incompatible later repeats.
    """
    reference = comparison_signature(runs[0]) if runs else None
    unmatched = []
    for run in runs:
        signature = comparison_signature(run)
        reasons = [key for key in reference if signature[key] != reference[key]]
        measurement = run['measurement']
        if run.get('exit_success') is not True:
            reasons.append('process_exit')
        if measurement.get('request_matched') is not True:
            reasons.append('requested_workload')
        minimum_seconds = measurement.get('minimum_measured_seconds') or 0.
        seconds = measurement.get('measured_loop_wall_seconds')
        if minimum_seconds > 0. and (not isinstance(seconds, (int, float))
                or not math.isfinite(seconds) or seconds < minimum_seconds):
            reasons.append('measured_duration_floor')
        audio = measurement.get('audio', {})
        if audio.get('playback_requested') and not audio.get('actual_audio_playback_observed'):
            reasons.append('requested_audio_not_verified')
        if audio.get('playback_requested'):
            window = object_or_empty(audio.get('measured_counter_window'))
            if (window.get('version') != 1 or any(
                    not valid_counter_delta(window.get(key))
                    for key in ('transport_underruns', 'vm_underrun_periods'))):
                reasons.append('measured_audio_counter_window_unavailable')
        if reasons:
            unmatched.append({'label': run['label'], 'different_or_invalid_fields': reasons})
    by_variant = {variant: [run for run in runs if run['variant'] == variant] for variant in variants}
    for variant, group in by_variant.items():
        if sorted(run['repeat'] for run in group) != list(range(1, expected_repeats + 1)):
            unmatched.append({'variant': variant, 'different_or_invalid_fields': ['repeat_set']})
    matched = bool(runs) and not unmatched
    result = {'matched': matched, 'all_repeats_checked': True, 'unmatched_runs': unmatched,
              'comparison_reference_run': runs[0]['label'] if runs else None,
              'comparison_reference_conditions': reference,
              'aggregation_policy': 'All-or-none: every included repeat must match the same workload and '
                  'observed dimensions/device/audio state; any mismatch suppresses all aggregate medians and speedups.',
              'variants': {}}
    for variant, group in by_variant.items():
        measurements = [run['measurement'] for run in group]
        means = [m['groups']['all_measured']['frame_ms']['mean_ms'] for m in measurements]
        durations = [m.get('measured_loop_wall_seconds') for m in measurements]
        value = {'run_labels': [run['label'] for run in group], 'repeated_run_frame_mean_ms': means,
                 'actual_measured_frames_per_run': [m['measured_frames'] for m in measurements],
                 'measured_loop_fps': [m.get('measured_loop_fps') for m in measurements],
                 'actual_measured_loop_seconds_per_run': durations,
                 'actual_measurement_duration_complete': all(duration is not None for duration in durations),
                 'p95_ms_per_run': [m['groups']['all_measured']['frame_ms']['p95_ms'] for m in measurements],
                 'p99_ms_per_run': [m['groups']['all_measured']['frame_ms']['p99_ms'] for m in measurements],
                 'actual_audio_playback_verified_per_run': [m['audio']['actual_audio_playback_observed'] for m in measurements],
                 'measured_audio_counter_windows_per_run': [m['audio'].get('measured_counter_window') for m in measurements],
                 'gpu_measurement_complete_per_run': [m.get('gpu_measurement_complete') for m in measurements],
                 'aggregate_included': matched}
        if matched:
            value.update(median_run_frame_mean_ms=statistics.median(means),
                         median_run_cost_equivalent_fps=1000 / statistics.median(means),
                         total_actual_measured_loop_seconds=sum(durations) if all(d is not None for d in durations) else None)
        result['variants'][variant] = value
    if matched and 'candidate' in by_variant:
        result['median_frame_cost_speedup'] = (result['variants']['baseline']['median_run_frame_mean_ms'] /
                                               result['variants']['candidate']['median_run_frame_mean_ms'])
    return result


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--app', type=Path, default=ROOT / 'output/macOS/MirrorPrismals.app')
    parser.add_argument('--candidate', type=Path)
    parser.add_argument('--baseline-env', action='append', default=[], metavar='KEY=0|1',
                        help='Allowlisted baseline optimization flag; repeat per flag')
    parser.add_argument('--candidate-env', action='append', default=[], metavar='KEY=0|1',
                        help='Allowlisted candidate optimization flag; creates a same-app candidate if --candidate is omitted')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--size', action='append', help='Logical WIDTHxHEIGHT, repeat for more sizes')
    parser.add_argument('--fullscreen', action='store_true',
                        help='Use the normal launcher fullscreen path; omit logical-size override and measure actual dimensions')
    parser.add_argument('--retina', choices=('0', '1'), default='0')
    parser.add_argument('--frames', type=int, default=600)
    parser.add_argument('--warmup', type=int, default=120)
    parser.add_argument('--cycle', type=int, default=32)
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--max-attempts', type=int, default=3,
                        help='Whole-session attempts per repeat, 1..3; only explicit recoverable pre-draw window interruptions retry')
    parser.add_argument('--min-seconds', type=float,
                        help='Minimum measured seconds after warmup; default30 for new benchmark,0 for legacy; hardcap20000frames/240s native')
    parser.add_argument('--legacy', action='store_true', help='Use existing four-frame warmup/single motion cycle')
    parser.add_argument('--audio', action='store_true', help='Opt-in native MIDI playback at master gain0.05; owned JACK lifecycle')
    parser.add_argument('--scenario', choices=('idle', 'motion'), default='motion')
    parser.add_argument('--conditions-note', default='', help='Known user activity, power or competing-job context')
    args = parser.parse_args()
    if not 8 <= args.frames <= 600 or not 1 <= args.warmup <= 120 or not 1 <= args.repeats <= 5:
        parser.error('frames8..600, warmup1..120, repeats1..5 required')
    if not 1 <= args.max_attempts <= 3:
        parser.error('max-attempts must be within1..3')
    if not 8 <= args.cycle <= 600 or args.cycle % 4:
        parser.error('cycle must be a multiple of four in8..600')
    if args.fullscreen and args.size:
        parser.error('--fullscreen measures actual monitor size; --size is for a separate windowed run')
    args.size = ['normal-fullscreen'] if args.fullscreen else (args.size or ['640x480', '1280x720'])
    if not args.fullscreen:
        for size in args.size:
            pieces = size.split('x')
            if len(pieces) != 2 or not all(p.isdigit() and 320 <= int(p) <= 2048 for p in pieces):
                parser.error('size must be WIDTHxHEIGHT within320..2048')
    if len(args.size) > 4:
        parser.error('at most four sizes per finite run')
    if args.audio and args.legacy:
        parser.error('--audio requires the new benchmark controls; it cannot be claimed with --legacy')
    if args.scenario == 'idle' and args.legacy:
        parser.error('Settled idle mode requires the new benchmark controls')
    args.min_seconds = (0. if args.legacy else 30.) if args.min_seconds is None else args.min_seconds
    if not math.isfinite(args.min_seconds) or not 0 <= args.min_seconds <= 120:
        parser.error('min-seconds must be finite within0..120')
    if args.legacy and args.min_seconds:
        parser.error('A duration floor requires the new benchmark controls')
    try:
        args.baseline_env = parse_variant_flags(args.baseline_env)
        args.candidate_env = parse_variant_flags(args.candidate_env)
    except ValueError as error:
        parser.error(str(error))
    return args


def main():
    args = parse_args()
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f')
    out = args.output or ROOT / 'output/checks' / ('workstation-repeated-' + stamp)
    out.mkdir(parents=True, exist_ok=False)
    apps = {'baseline': args.app.resolve()}
    if args.candidate or args.candidate_env:
        apps['candidate'] = (args.candidate or args.app).resolve()
    variant_flags = {'baseline': args.baseline_env, 'candidate': args.candidate_env}
    report = {'schema': 1, 'status': 'running', 'output': str(out.resolve()),
              'builds': {}, 'runs': [], 'attempts': [], 'comparisons': [], 'hardware': hardware(),
              'attempt_policy': 'Each attempt is a fresh native process, graphics/audio session, document, warmup and measured loop. '
                  'Only complete uninterrupted attempts enter runs/comparisons. Every failed attempt remains in attempts with its own directory/logs; '
                  'no frames are skipped and no samples or elapsed time are pooled across attempts.',
              'conditions_note': args.conditions_note, 'installed': False,
              'measurement_design': ('Repeated runs requiring both minimum frames and minimum measured duration after warmup, '
                  'ending at a complete motion cycle; finite cap20000measuredframes/240s native. No thermal steady-state claim.'
                  if args.min_seconds else 'Short repeated fixed-frame runs; no minimum duration or thermal steady-state claim.'),
              'duration_example': '600 frames alone last about10seconds at60FPS or3seconds at200FPS. '
                  'A positive duration floor continues replay after that count; actual elapsed time is recorded per run.',
              'variant_flags': {name: variant_flags[name] for name in apps},
              'request': {'frames': args.frames, 'warmup': 4 if args.legacy else args.warmup,
                          'cycle': None if args.legacy else args.cycle, 'repeats': args.repeats,
                          'sizes': args.size, 'retina': args.retina, 'legacy': args.legacy,
                          'audio': args.audio, 'scenario': args.scenario, 'fullscreen': args.fullscreen,
                          'minimum_measured_seconds': args.min_seconds,
                          'maximum_attempts_per_repeat': args.max_attempts,
                          'maximum_process_seconds_per_repeat': 300 * args.max_attempts,
                          'maximum_measured_frames': 20_000 if args.min_seconds else args.frames,
                          'process_deadline_seconds': 300},
              'measurement_limits': [
                  'Native MIDI playback is opt-in. Callback/load/dropout claims require observed native samples. '
                  'Mixer meter/fader geometry remains static; no control gesture fixture or input latency is generated.',
                  'Panel geometry expands/resizes; GLFW window itself is nonresizable and is not resized during replay.',
                  '1280x720 is an additional window-size probe, not a claim about the user\'s actual session size.',
                  'Native GPU totals span bake+resolve+render including gaps; per-pass attribution is reported only when emitted by the binary.',
                  'render_present_ms includes surface acquisition, encoding, submission, synchronous GPU wait and presentation.',
                  'FIFO present and synchronous wait are source-confirmed policies; display scanout/event latency is not measured.',
                  'Frame-cost equivalent FPS excludes bookkeeping outside the timer; complete-loop FPS is separate when available.',
                  'Short finite repeated runs reveal variation but do not prove thermal steady state.',
                  'Legacy mode runs one motion cycle; long legacy runs contain mostly settled frames and cannot establish sustained motion performance.',
                  'Native capture is only the initial scene; visual regression/animated appearance still requires separate probes.'],
              'source_policy': {'present_mode': 'WGPUPresentMode_Fifo',
                                'vsync_requested': True, 'actual_scanout_hz': None,
                                'render_source_sha256': digest(ROOT / 'Render/webgpu.rs'),
                                'window_source_sha256': digest(ROOT / 'Render/window.rs')}}

    def save():
        (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')

    try:
        report['builds'] = {name: identity(app) for name, app in apps.items()}
        report['same_executable_hash_across_variants'] = len({
            build['executable_sha256'] for build in report['builds'].values()}) == 1
        report['same_art_and_shader_hashes_across_variants'] = all(
            build['art_and_shader_hashes'] == report['builds']['baseline']['art_and_shader_hashes']
            for build in report['builds'].values())
        save()
        for size in args.size:
            for repeat in range(args.repeats):
                # Alternate order so candidate is not always the warmer run.
                order = list(apps)
                if repeat % 2:
                    order.reverse()
                for name in order:
                    label = f'{size}-repeat{repeat + 1}-{name}'
                    session_directory = out / label
                    session_directory.mkdir()
                    command = [str(apps[name] / 'Contents/MacOS/wormwood-engine'),
                               '--benchmark-workstation', 'WebGPU', '--frames', str(args.frames)]
                    for attempt_number in range(1, args.max_attempts + 1):
                        destination = session_directory / f'attempt-{attempt_number}'
                        destination.mkdir()
                        env = {k: v for k, v in os.environ.items() if not k.startswith('WORMWOOD_')}
                        env.update(WORMWOOD_ASSET_ROOT=str(apps[name] / 'Contents/Resources/EngineAssets'),
                                   WORMWOOD_OUTPUT_ROOT=str(destination), WORMWOOD_FULLSCREEN=str(int(args.fullscreen)),
                                   WORMWOOD_WORKSTATION_RETINA=args.retina,
                                   WORMWOOD_PERFORMANCE='1', WORMWOOD_WORKSTATION_CPU_STAGES='1')
                        if not args.fullscreen:
                            env['WORMWOOD_WORKSTATION_SIZE'] = size
                        if not args.legacy:
                            env.update(WORMWOOD_WORKSTATION_BENCHMARK_WARMUP=str(args.warmup),
                                       WORMWOOD_WORKSTATION_BENCHMARK_CYCLE=str(args.cycle),
                                       WORMWOOD_WORKSTATION_BENCHMARK_AUDIO=str(int(args.audio)),
                                       WORMWOOD_WORKSTATION_BENCHMARK_SCENARIO=args.scenario,
                                       WORMWOOD_WORKSTATION_BENCHMARK_MIN_SECONDS=str(args.min_seconds))
                        env.update(variant_flags[name])
                        run = {'label': label, 'variant': name, 'repeat': repeat + 1, 'size_requested': size,
                               'attempt': attempt_number, 'attempt_id': f'{label}/attempt-{attempt_number}',
                               'output': str(destination), 'before': environment_snapshot(),
                               'included_in_comparison': False,
                               'requested_variant_flags': variant_flags[name],
                               'executable_sha256': report['builds'][name]['executable_sha256'],
                               'effective_environment': {k: v for k, v in env.items() if k.startswith('WORMWOOD_')}}
                        report['attempts'].append(run)
                        save()
                        print(run['attempt_id'], flush=True)
                        started = time.monotonic()
                        try:
                            run_checked(command, cwd=ROOT, env=env, log=destination / 'native.log', timeout=300)
                            run.update(exit_success=True, exit_code=0)
                        except subprocess.CalledProcessError as error:
                            run.update(exit_success=False, exit_code=error.returncode,
                                       process_failure_kind='exit_status', error=str(error))
                        except subprocess.TimeoutExpired as error:
                            run.update(exit_success=False, process_failure_kind='timeout', error=str(error))
                        except Exception as error:
                            run.update(exit_success=False, process_failure_kind='launch_or_harness_error', error=str(error))
                        run['process_wall_seconds_including_startup_teardown'] = time.monotonic() - started
                        run['after'] = environment_snapshot()
                        run['measurement'] = summarize(destination, args.legacy, args.warmup, args.cycle,
                                                        args.frames, args.audio, args.scenario, args.min_seconds)
                        run['disposition'] = attempt_disposition(run, attempt_number, args.max_attempts)
                        if run['disposition']['action'] == 'include':
                            run['included_in_comparison'] = True
                            report['runs'].append(run)
                        save()
                        (destination / 'attempt-report.json').write_text(json.dumps(run, indent=2) + '\n')
                        if run['disposition']['action'] == 'include':
                            break
                        if run['disposition']['action'] == 'retry_fresh_session':
                            print(run['attempt_id'] + ': ' + run['disposition']['reason']
                                  + '; restarting the whole native session with fresh warmup', flush=True)
                            continue
                        raise RuntimeError(run['attempt_id'] + ': ' + run['disposition']['reason']
                            + ('; bounded attempt limit reached' if run['disposition'].get('attempt_limit_reached') else '')
                            + '; full attempt evidence preserved at ' + str(destination))
        for size in args.size:
            summary = repeat_comparison([run for run in report['runs'] if run['size_requested'] == size],
                                        apps, args.repeats)
            summary['size_requested'] = size
            if 'candidate' in apps:
                art = [report['builds'][name]['art_and_shader_hashes'] for name in ('baseline', 'candidate')]
                summary['authored_art_identical'] = {k: v for k, v in art[0].items() if k.startswith('Art/')} == {
                    k: v for k, v in art[1].items() if k.startswith('Art/')}
            report['comparisons'].append(summary)
        report['identities_unchanged'] = report['builds'] == {name: identity(app) for name, app in apps.items()}
        if not report['identities_unchanged']:
            raise RuntimeError('An app changed during the run; comparisons are not validated')
        report['status'] = ('completed_graphics_and_native_audio' if args.audio and all(
            run['measurement']['audio']['actual_audio_playback_observed'] for run in report['runs'])
            else 'completed_graphics_audio_unavailable' if args.audio else 'completed_graphics_only')
        if any(not comparison['matched'] for comparison in report['comparisons']):
            report['status'] = 'completed_unmatched_runs_no_aggregate'
        elif any(not run['measurement']['gpu_measurement_complete'] for run in report['runs']):
            report['status'] = 'completed_native_workload_gpu_timing_unavailable_or_incomplete'
    except BaseException as error:
        report.update(status='incomplete', error=str(error))
        raise
    finally:
        save()
        print(json.dumps({'status': report['status'], 'report': str(out / 'report.json')}), flush=True)


if __name__ == '__main__':
    main()
