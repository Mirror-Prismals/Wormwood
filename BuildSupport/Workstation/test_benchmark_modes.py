"""Coverage gates for the five-mode suite; no native processes are launched."""
import argparse
from pathlib import Path
import unittest
from benchmark_modes import command_env, summarize_daw, summarize_game


def fixture(mode='ar'):
    count = {'complete': True, 'delta': 0, 'start_lifetime_total': 7, 'end_lifetime_total': 7}
    b = {'complete': True, 'warmup_frames': 120, 'fixed_simulation_step_seconds': None,
         'measured_loop_wall_seconds': 30, 'measured_frames': 600, 'resized_during_measurement': False,
         'last_window': {'device': {'workstation_shader': {'selected': 'baked_images'}}},
         'device_last': {'device': 'GPU'}, 'dimensions': [1920, 1200],
         'measured_loop_frames_per_second': 20, 'complete_loop_timing': {},
         'motion_input': {'gate': False, 'changes': False, 'events': [{}, {}],
                          'frames': [{'frame': i, 'at': i*.02, 'extensions': [i/100]} for i in range(20)]},
         'wearable_verification': [{'player': [0,0,0]}, {'player': [1,0,0]}]}
    m = {'benchmark': b, 'embedded_evidence': {'embedded': True, 'validated_frames': 720,
         'computer_screen': mode == 'computer', 'eyewear': mode if mode in ('ar','vr') else None,
         'observer_motion': mode == 'vr', 'audio_rays': False},
         'gpu_measurement': {'supported': True, 'complete': True},
         'benchmark_audio': {'measured_counter_window': {'transport_underruns': count, 'vm_underrun_periods': count}}}
    return {'entities_remaining': 0, 'combined_workstation': m}


class Gates(unittest.TestCase):
    def test_native_paths_do_not_depend_on_process_working_directory(self):
        args = argparse.Namespace(game_frames=2400, warmup=120, seconds=30)
        _, env = command_env('computer', args, Path('relative/run'), Path('relative/assets'))
        for name in ('WORMWOOD_OUTPUT_ROOT', 'WORMWOOD_ASSET_ROOT', 'WORMWOOD_SETTINGS_PATH'):
            self.assertTrue(Path(env[name]).is_absolute(), name)

    def test_wearables_do_not_enable_verification_rays_or_physical_screen(self):
        args = argparse.Namespace(game_frames=2400, warmup=120, seconds=30)
        for mode in ('ar','vr'):
            _, env = command_env(mode, args, Path('/tmp/run'), Path('/tmp/assets'))
            self.assertEqual(env['WORMWOOD_WORKSTATION_BENCHMARK_EYEWEAR'], mode)
            self.assertNotIn('WORMWOOD_WEARABLE_VERIFY', env)
            self.assertNotIn('WORMWOOD_COMPUTER_SCREEN_VERIFY', env)

    def test_wrong_observer_cannot_be_presented_as_requested_mode(self):
        with self.assertRaisesRegex(ValueError, 'physical-screen'):
            summarize_daw(fixture('computer'), 'ar', 30, 120)

    def test_fixed_step_result_cannot_pass_as_interactive_motion(self):
        r = fixture(); r['combined_workstation']['benchmark']['fixed_simulation_step_seconds'] = 1/30
        with self.assertRaisesRegex(ValueError, 'fixed-step'):
            summarize_daw(r, 'ar', 30, 120)

    def test_vr_requires_observed_player_motion(self):
        r = fixture('vr');r['combined_workstation']['benchmark']['wearable_verification'] = [{'player': [0,0,0]}]*3
        with self.assertRaisesRegex(ValueError, 'never moved'):
            summarize_daw(r, 'vr', 30, 120)

    def test_missing_drawn_frame_cannot_inflate_motion_fps(self):
        r = fixture();r['combined_workstation']['benchmark']['motion_input']['frames'][5]['frame'] = 7
        with self.assertRaisesRegex(ValueError, 'Missing drawn'):
            summarize_daw(r, 'ar', 30, 120)

    def test_audio_lifetime_total_is_not_used_as_delta(self):
        r = fixture();window = r['combined_workstation']['benchmark_audio']['measured_counter_window']
        window['transport_underruns'] = {'complete': True, 'start_lifetime_total': 7, 'end_lifetime_total': 8, 'delta': 0}
        with self.assertRaisesRegex(ValueError, 'audio counter'):
            summarize_daw(r, 'ar', 30, 120)

    def test_game_excludes_warmup_and_includes_pacing(self):
        native = {'entities_remaining': 0, 'combined_workstation': None}
        timing = {'dropped_frames': 0, 'frames': [[i, 50., 5., 10., True, True, i] for i in range(1,151)]}
        m = summarize_game(native, timing, 120, 1)
        self.assertEqual(m['measured_frames'], 20)
        self.assertEqual(m['whole_loop_fps'], 20)
        with self.assertRaisesRegex(ValueError, 'too short'):
            summarize_game(native, timing, 120, 2)

    def test_valid_motion_uses_loop_start_intervals(self):
        r = summarize_daw(fixture(), 'ar', 30, 120)
        self.assertAlmostEqual(r['moving']['fps'], 50)
        self.assertEqual(r['transport_underruns'], 0)


if __name__ == '__main__':
    unittest.main()
