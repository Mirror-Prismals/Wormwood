#!/usr/bin/env python3
"""Inspect or offline-process an Intel Mac VST2, without native plugin loading."""
import argparse
import json
import math
import platform
import sys
import struct
import wave

from macho import CompatibilityError, MachO, resolve_binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugin", help="Mac .vst bundle or its Mach-O executable")
    parser.add_argument("--architecture", choices=("x86_64", "i386"), default="x86_64",
                        help="Guest architecture; i386 currently supports read-only inspection")
    parser.add_argument("--process", action="store_true", help="Execute using Unicorn and process silent audio offline")
    parser.add_argument("--trace-imports", action="store_true", help="Diagnose initialization: stop at the first accessed missing function/data import")
    parser.add_argument("--trace-audio", action="store_true", help="Offline diagnostic audio only; trap if an unresolved import is exercised")
    parser.add_argument("--signal", choices=("silence", "sine"), default="silence", help="Offline input: silence or a 440 Hz sine at 0.125 amplitude")
    parser.add_argument("--output-wav", help="Write the offline output as a new 16-bit PCM WAV file")
    parser.add_argument("--parameter", action="append", default=[], metavar="INDEX=VALUE",
                        help="Set a normalized VST parameter before processing; repeat as needed")
    parser.add_argument("--guest-library", action="append", default=[],
                        help="Explicit guest Mach-O dylib; repeat in dependency initialization order")
    parser.add_argument("--blocks", type=int, default=4)
    parser.add_argument("--frames", type=int, default=256)
    parser.add_argument("--startup-seconds", type=float, default=2,
                        help="Per-call startup time budget, 0.1..30 seconds (audio/cleanup stay at 2 seconds)")
    args = parser.parse_args()
    if sum((args.process, args.trace_imports, args.trace_audio)) > 1:
        parser.error("Choose only one of --process, --trace-imports, or --trace-audio")
    if not 1 <= args.blocks <= 100 or not 1 <= args.frames <= 8192:
        parser.error("blocks must be 1..100 and frames 1..8192")
    parameters = []
    try:
        for assignment in args.parameter:
            index, value = assignment.split("=", 1)
            index, value = int(index), float(value)
            if index < 0 or not math.isfinite(value) or not 0 <= value <= 1:
                raise ValueError("out of range")
            parameters.append((index, value))
    except ValueError:
        parser.error("Parameters must be INDEX=VALUE with a nonnegative index and normalized value 0..1")
    if args.output_wav and not (args.process or args.trace_audio):
        parser.error("--output-wav requires --process or --trace-audio")
    if parameters and not (args.process or args.trace_audio):
        parser.error("--parameter requires --process or --trace-audio")
    worker = None
    try:
        image_type = MachO
        if args.architecture == "i386":
            from macho32 import MachO32
            image_type = MachO32
        image = image_type.from_path(args.plugin)
        libraries = [image_type.from_path(path) for path in args.guest_library]
        report = image.report()
        report["guest_libraries"] = [library.report() for library in libraries]
        report.update(binary=str(resolve_binary(args.plugin)), host_architecture=platform.machine(), executed=False)
        if args.process or args.trace_imports or args.trace_audio:
            from runtime import Runtime
            worker = Runtime(image, block_size=args.frames, trace_imports=args.trace_imports or args.trace_audio,
                             libraries=libraries, startup_seconds=args.startup_seconds)
            if args.trace_imports:
                report.update(initialization_diagnostics=worker.diagnostics(),
                              audio_compatibility_verified=False)
                worker.close()
                worker = None
                print(json.dumps(report, indent=2))
                return 0
            applied_parameters = []
            for index, value in parameters:
                worker.set_parameter(index, value)
                applied_parameters.append({"index": index, "requested": value, "returned": worker.get_parameter(index)})
            peak = 0.0
            squared = 0.0
            samples = 0
            pcm = bytearray()
            clipped_samples = 0
            largest_difference = 0.0
            process = worker.diagnose_audio if args.trace_audio else worker.process
            for _ in range(args.blocks):
                signal = [0.125 * math.sin(2 * math.pi * 440 * (worker.position + i) / worker.sample_rate)
                          if args.signal == "sine" else 0.0 for i in range(args.frames)]
                output = process([list(signal) for _ in range(worker.inputs)], args.frames)
                if worker.inputs == worker.outputs:
                    largest_difference = max(largest_difference, max(abs(v - signal[i]) for ch in output for i, v in enumerate(ch)))
                if args.output_wav:
                    interleaved = [v for frame in zip(*output) for v in frame]
                    clipped_samples += sum(abs(v) > 1 for v in interleaved)
                    pcm.extend(struct.pack(f"<{len(interleaved)}h", *(max(-32768, min(32767, round(v * 32768))) for v in interleaved)))
                squared += sum(v * v for ch in output for v in ch)
                samples += sum(len(ch) for ch in output)
                peak = max(peak, max(abs(v) for ch in output for v in ch))
            report.update(executed=True, backend="Unicorn x86_64", rosetta_used=False,
                          inputs=worker.inputs, outputs=worker.outputs, parameters=worker.params,
                          processed_frames=worker.position, peak=peak, rms=math.sqrt(squared / samples) if samples else 0.0,
                          input_signal=args.signal, diagnostic_audio=args.trace_audio,
                          applied_parameters=applied_parameters, maximum_difference_from_input=largest_difference if worker.inputs == worker.outputs else None,
                          general_plugin_compatibility_verified=False,
                          initialization_diagnostics=worker.diagnostics(),
                          host_callbacks=worker.host_calls,
                          unsupported_host_callbacks=sorted(worker.unknown_host_calls))
            worker.close()
            report["cleanup_completed"] = True
            report["final_diagnostics"] = worker.diagnostics()
            if args.output_wav:
                with open(args.output_wav, "xb") as output_file:
                    with wave.open(output_file, "wb") as wav:
                        wav.setnchannels(worker.outputs)
                        wav.setsampwidth(2)
                        wav.setframerate(int(worker.sample_rate))
                        wav.writeframes(pcm)
                report.update(output_wav=args.output_wav, wav_clipped_samples=clipped_samples)
            worker = None
        print(json.dumps(report, indent=2))
        return 0
    except (CompatibilityError, OSError, ValueError) as exc:
        print(json.dumps({"error": str(exc), "executed_successfully": False,
                          "diagnostics": getattr(exc, "details", worker.diagnostics() if worker else None)}, indent=2), file=sys.stderr)
        return 1
    finally:
        if worker is not None:
            worker.close(suppress=True)


if __name__ == "__main__":
    raise SystemExit(main())
