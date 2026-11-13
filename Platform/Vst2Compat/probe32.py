#!/usr/bin/env python3
"""Offline i386 initializer probe; reports expected compatibility stops as data."""
import argparse
import json
import math
import time

from macho import CompatibilityError
from macho32 import MachO32


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('plugin')
    parser.add_argument('--initialize', action='store_true')
    parser.add_argument('--open', action='store_true', help='Initialize and invoke the VST entry/open callbacks')
    parser.add_argument('--resource-root', help='Read-only resource mirror, using the same layout as the 64-bit worker')
    parser.add_argument('--startup-seconds', type=float, default=15, help='Bounded startup call allowance, 0.1..30 seconds')
    parser.add_argument('--parameters', action='store_true', help='Open and inspect VST parameter metadata')
    parser.add_argument('--process', action='store_true', help='Render 16 offline stereo/mono sine blocks; does not prove a wet effect')
    args = parser.parse_args()
    image = MachO32.from_path(args.plugin)
    report = image.report()
    report.update(executed=False, audio_compatibility_verified=False, rosetta_used=False)
    if args.initialize or args.open or args.parameters or args.process:
        from runtime32 import Runtime32
        runtime = None
        try:
            runtime = Runtime32(image, resource_root=args.resource_root, startup_seconds=args.startup_seconds)
            report['executed'] = True
            runtime.initialize()
            report.update(status='initializers_completed', diagnostics=runtime.diagnostics())
            if args.open or args.parameters or args.process:
                report['plugin'] = runtime.open_plugin()
                report.update(status='plugin_opened', diagnostics=runtime.diagnostics())
            if args.parameters:
                report['parameters'] = runtime.parameter_info()
                report.update(status='parameters_read', diagnostics=runtime.diagnostics())
            if args.process:
                runtime.start_audio()
                started = time.monotonic()
                peak, energy = 0., 0.
                for block in range(16):
                    signal = [.2 * math.sin(2 * math.pi * 440 * (block * 256 + i) / 48000)
                              for i in range(256)]
                    output = runtime.process([signal] * runtime.inputs, 256)
                    for channel in output:
                        peak = max(peak, max(map(abs, channel)))
                        energy += sum(value * value for value in channel)
                report.update(status='audio_rendered', diagnostics=runtime.diagnostics(),
                              audio=dict(frames=4096, sample_rate=48000, peak=peak,
                                         rms=math.sqrt(energy / (4096 * runtime.outputs)),
                                         render_seconds=time.monotonic() - started))
        except CompatibilityError as exc:
            report.update(status='blocked', error=str(exc),
                          diagnostics=getattr(exc, 'details', runtime.diagnostics() if runtime else None))
        finally:
            if runtime is not None:
                try:
                    report['guest_cleanup_skipped'] = runtime.failed
                    runtime.close()
                    report['cleanup_completed'] = True
                    report['cleanup_diagnostics'] = runtime.diagnostics()
                except CompatibilityError as exc:
                    report.update(cleanup_completed=False, cleanup_error=str(exc),
                                  cleanup_diagnostics=getattr(exc, 'details', None))
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
