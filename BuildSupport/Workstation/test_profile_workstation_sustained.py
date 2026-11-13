"""Pure report-gating tests; never launch a renderer, audio server or subprocess."""
import importlib.util
import copy
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('profile_workstation_sustained',
    Path(__file__).with_name('profile_workstation_sustained.py'))
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


def fixture():
    runs = []
    for repeat in range(1, 4):
        for variant in ('baseline', 'candidate'):
            runs.append({'label': f'{variant}-{repeat}', 'variant': variant, 'repeat': repeat,
                'exit_success': True,
                'effective_environment': {'WORMWOOD_WORKSTATION_RETINA': '0',
                    'WORMWOOD_WORKSTATION_PIXEL_TRANSPORT': str(int(variant == 'candidate'))},
                'measurement': {'request_matched': True, 'scenario': 'motion',
                    'warmup_frames': 120, 'cycle_frames': 32, 'measured_frames': 600,
                    'framebuffer_dimensions': [640, 480], 'logical_dimensions_observed': [[640, 480]],
                    'gpu_method': 'async_timestamp', 'gpu_supported': True,
                    'measured_loop_wall_seconds': 10. + repeat, 'measured_loop_fps': 50.,
                    'device_last': {'api': 'WebGPU', 'device': 'test GPU'},
                    'groups': {'all_measured': {'frame_ms': {'mean_ms': 20., 'p95_ms': 24., 'p99_ms': 27.}}},
                    'audio': {'playback_requested': True, 'actual_audio_playback_observed': True,
                        'measured_counter_window': {'version': 1, 'boundary_mode': 'synchronous_workstation_draw',
                            'transport_underruns': {'complete': True, 'start_lifetime_total': 12,
                                'end_lifetime_total': 15, 'delta': 3},
                            'vm_underrun_periods': {'complete': True, 'start_lifetime_total': 20,
                                'end_lifetime_total': 20, 'delta': 0}},
                        'native_device_and_callback_info': {'output_device_uid': 'test output',
                            'sample_rate_hz': 48000, 'buffer_frames': 256, 'buffer_budget_ms': 256/48,
                            'mode': 'embedded_chuck', 'realtime': True}}}})
    return runs


class ReportGating(unittest.TestCase):
    def compare(self, runs):
        return profile.repeat_comparison(runs, ('baseline', 'candidate'), 3)

    def assert_suppressed(self, report, label, field):
        self.assertFalse(report['matched'])
        self.assertNotIn('median_frame_cost_speedup', report)
        self.assertTrue(any(run.get('label') == label and field in run['different_or_invalid_fields']
                            for run in report['unmatched_runs']))
        for variant in report['variants'].values():
            self.assertFalse(variant['aggregate_included'])
            self.assertNotIn('median_run_frame_mean_ms', variant)

    def test_all_repeats_match_with_intended_flag_difference_and_actual_durations(self):
        report = self.compare(fixture())
        self.assertTrue(report['matched'])
        self.assertEqual(report['median_frame_cost_speedup'], 1.)
        self.assertEqual(report['variants']['candidate']['actual_measured_loop_seconds_per_run'], [11.,12.,13.])
        self.assertEqual(report['variants']['candidate']['total_actual_measured_loop_seconds'], 36.)

    def test_later_dimension_drift_suppresses_whole_aggregate(self):
        runs = fixture()
        runs[3]['measurement']['framebuffer_dimensions'] = [1280, 960]
        self.assert_suppressed(self.compare(runs), 'candidate-2', 'framebuffer_dimensions')

    def test_later_audio_failure_preserves_individual_verification(self):
        runs = fixture()
        runs[5]['measurement']['audio']['actual_audio_playback_observed'] = False
        report = self.compare(runs)
        self.assert_suppressed(report, 'candidate-3', 'actual_audio_playback_observed')
        self.assertEqual(report['variants']['candidate']['actual_audio_playback_verified_per_run'], [True,True,False])

    def test_later_audio_device_change_is_unmatched(self):
        runs = fixture()
        runs[2]['measurement']['audio']['native_device_and_callback_info']['buffer_frames'] = 512
        self.assert_suppressed(self.compare(runs), 'baseline-2', 'native_audio_configuration')

    def test_missing_repeat_and_failed_process_cannot_be_aggregated(self):
        runs = fixture()
        self.assertFalse(self.compare(runs[:-1])['matched'])
        runs[4]['exit_success'] = False
        self.assert_suppressed(self.compare(runs), 'baseline-3', 'process_exit')

    def test_quality_environment_is_not_an_allowed_variant(self):
        runs = fixture()
        runs[3]['effective_environment']['WORMWOOD_WORKSTATION_RETINA'] = '1'
        self.assert_suppressed(self.compare(runs), 'candidate-2', 'environment')

    def test_flag_allowlist_rejects_unrelated_keys_values_and_duplicates(self):
        key = 'WORMWOOD_WORKSTATION_PIXEL_TRANSPORT'
        self.assertEqual(profile.parse_variant_flags([key + '=1']), {key: '1'})
        resolve = 'WORMWOOD_WORKSTATION_RESOLVE_REUSE'
        self.assertEqual(profile.parse_variant_flags([resolve + '=1']), {resolve: '1'})
        for values in [['WORMWOOD_WORKSTATION_RETINA=0'], [key + '=false'], [key + '=0', key + '=1']]:
            with self.assertRaises(ValueError):
                profile.parse_variant_flags(values)

    def test_duration_floor_does_not_require_identical_frame_counts(self):
        runs = fixture()
        for run in runs:
            measurement = run['measurement']
            measurement.update(minimum_requested_frames=120, minimum_measured_seconds=30.,
                maximum_measured_frames=20_000, measured_loop_wall_seconds=30.1,
                measured_frames=960 if run['variant']=='baseline' else 1920)
        self.assertTrue(self.compare(runs)['matched'])
        runs[3]['measurement']['measured_loop_wall_seconds'] = 29.9
        self.assert_suppressed(self.compare(runs), 'candidate-2', 'measured_duration_floor')

    def test_all_audio_requests_failing_do_not_become_a_matched_audio_aggregate(self):
        runs = fixture()
        for run in runs:
            run['measurement']['audio']['actual_audio_playback_observed'] = False
        self.assert_suppressed(self.compare(runs), 'baseline-1', 'requested_audio_not_verified')

    def test_later_missing_or_invalid_audio_window_suppresses_aggregate(self):
        for bad in [None, {'complete': True, 'delta': None},
                    {'complete': True, 'start_lifetime_total': 5, 'end_lifetime_total': 4, 'delta': 0},
                    {'complete': True, 'start_lifetime_total': 0, 'end_lifetime_total': 1, 'delta': True}]:
            runs = fixture()
            runs[5]['measurement']['audio']['measured_counter_window']['transport_underruns'] = bad
            self.assert_suppressed(self.compare(runs), 'candidate-3', 'measured_audio_counter_window_unavailable')


def interrupted_attempt(reason='zero_framebuffer'):
    return {'exit_success': False, 'exit_code': 1, 'process_failure_kind': 'exit_status',
        'measurement': {'request_matched': False, 'native_complete': False,
            'native_end_reason': 'benchmark_nondrawable', 'native_errors': {},
            'last_window': {'drawn': False, 'closed': False, 'width': 1920, 'height': 1200,
                'dimensions_source': 'previous', 'draw_status': {'reason': reason,
                    'recoverable': True, 'attempted': False, 'drawable_dimensions': None}},
            'measured_frames': 74, 'measured_loop_wall_seconds': 8.1,
            'audio': {'playback_evidence': {}}}}


class AttemptRecovery(unittest.TestCase):
    def test_only_explicit_window_gates_restart_a_fresh_attempt(self):
        for reason in profile.RECOVERABLE_DRAW_REASONS:
            result = profile.attempt_disposition(interrupted_attempt(reason), 1, 3)
            self.assertEqual(result['action'], 'retry_fresh_session')
            self.assertTrue(result['samples_discarded_from_comparison'])
            self.assertIn(reason, result['reason'])
            self.assertNotIn('--legacy', result['reason'])

    def test_unknown_closed_attempted_and_fatal_conditions_fail_closed(self):
        original = interrupted_attempt()
        variants = []
        for key, value in [('reason', 'unknown'), ('reason', 'surface_timeout'), ('reason', []),
                           ('recoverable', False), ('recoverable', 'true'), ('attempted', True)]:
            run = copy.deepcopy(original)
            run['measurement']['last_window']['draw_status'][key] = value
            variants.append(run)
        for status in [None, 'zero_framebuffer']:
            run = copy.deepcopy(original)
            run['measurement']['last_window']['draw_status'] = status
            variants.append(run)
        for key, value in [('native_errors', {'gpu_error': 'device lost'}),
                           ('native_end_reason', 'window_closed')]:
            run = copy.deepcopy(original)
            run['measurement'][key] = value
            variants.append(run)
        for key, value in [('process_failure_kind', 'timeout'), ('exit_code', -9)]:
            run = copy.deepcopy(original)
            run[key] = value
            variants.append(run)
        run = copy.deepcopy(original)
        run['measurement']['audio']['playback_evidence']['runtime_error'] = 'audio route lost'
        variants.append(run)
        for run in variants:
            self.assertEqual(profile.attempt_disposition(run, 1, 3)['action'], 'fail')

    def test_attempt_limit_is_finite_and_partial_samples_remain_excluded(self):
        result = profile.attempt_disposition(interrupted_attempt(), 3, 3)
        self.assertEqual(result['action'], 'fail')
        self.assertTrue(result['attempt_limit_reached'])
        self.assertTrue(result['samples_discarded_from_comparison'])

    def test_fresh_complete_attempt_is_included_independently_of_failed_attempt(self):
        failed = interrupted_attempt()
        original = copy.deepcopy(failed)
        completed = {'exit_success': True, 'measurement': {'request_matched': True,
            'measured_frames': 512, 'measured_loop_wall_seconds': 30.2}}
        self.assertEqual(profile.attempt_disposition(failed, 1, 3)['action'], 'retry_fresh_session')
        self.assertEqual(profile.attempt_disposition(completed, 2, 3)['action'], 'include')
        self.assertEqual(failed, original)
        self.assertEqual(completed['measurement']['measured_frames'], 512)

    def test_duration_or_control_failure_has_specific_evidence_and_is_not_retryable(self):
        run = {'exit_success': True, 'measurement': {'request_matched': False,
            'native_end_reason': 'frame_limit', 'benchmark_controls_version': 2,
            'measured_frames': 120, 'measured_loop_wall_seconds': 10., 'native_complete': False}}
        result = profile.attempt_disposition(run, 1, 3)
        self.assertEqual(result['action'], 'fail')
        self.assertIn('measured_seconds=10.0', result['reason'])
        self.assertNotIn('unsupported', result['reason'])
        self.assertNotIn('--legacy', result['reason'])


if __name__ == '__main__':
    unittest.main()
