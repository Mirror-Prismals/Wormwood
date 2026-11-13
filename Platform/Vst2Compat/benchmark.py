#!/usr/bin/env python3
"""Measure steady-state worker processing and round-trip time, excluding startup."""
import argparse
import json
import math
from pathlib import Path
import platform
import struct
import time
from client import Client


def stats(values):
    values = sorted(values)
    return {"mean_ms": sum(values) / len(values) * 1000,
            "p50_ms": values[len(values) // 2] * 1000,
            "p95_ms": values[math.ceil(len(values) * 0.95) - 1] * 1000,
            "max_ms": values[-1] * 1000}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugin")
    parser.add_argument("--guest-library", action="append", default=[])
    parser.add_argument("--parameter", action="append", default=[])
    parser.add_argument("--blocks", type=int, default=64)
    parser.add_argument("--frames", default="128,256,512,1024")
    parser.add_argument("--experimental", action="store_true")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    sizes = [int(v) for v in args.frames.split(",")]
    if not sizes or len(sizes) > 8 or not all(1 <= v <= 8192 for v in sizes) or not 8 <= args.blocks <= 512:
        parser.error("Use 1..8 block sizes of 1..8192 frames and 8..512 blocks")
    report = {"host_architecture": platform.machine(), "results": [], "realtime_qualified": False}
    try:
        with Client() as worker:
            metadata, _ = worker.request("open", timeout=60, plugin=args.plugin,
                                         libraries=args.guest_library, experimental=args.experimental,
                                         block_size=max(sizes), startup_seconds=15)
            report["plugin"] = metadata
            for assignment in args.parameter:
                index, value = assignment.split("=", 1)
                worker.request("set_parameter", index=int(index), value=float(value))
            # Project state must survive serialization and a subsequent change.
            before, _ = worker.request("parameters")
            saved, _ = worker.request("save_state")
            if before["parameters"]:
                original = before["parameters"][0]["value"]
                worker.request("set_parameter", index=0, value=0.25 if original != 0.25 else 0.75)
            restored, _ = worker.request("load_state", state=saved["state"])
            if len(before["parameters"]) != len(restored["parameters"]) or any(
                    abs(a["value"] - b["value"]) > 1e-6 for a, b in zip(before["parameters"], restored["parameters"])):
                raise RuntimeError("State round trip changed parameter values")
            report["state_round_trip"] = True
            position = 0
            for frames in sizes:
                processing, round_trip = [], []
                peak = 0.0
                for block in range(args.blocks + 8):
                    signal = [0.125 * math.sin(2 * math.pi * 440 * (position + i) / metadata["sample_rate"]) for i in range(frames)]
                    position += frames
                    payload = struct.pack(f"<{frames * metadata['inputs']}f", *(signal * metadata["inputs"]))
                    start = time.perf_counter()
                    result, output = worker.request("process", frames=frames, audio=payload)
                    elapsed = time.perf_counter() - start
                    if block >= 8:
                        processing.append(result["process_seconds"])
                        round_trip.append(elapsed)
                        samples = struct.unpack(f"<{frames * metadata['outputs']}f", output)
                        peak = max(peak, max(abs(v) for v in samples))
                deadline = frames / metadata["sample_rate"]
                report["results"].append({"frames": frames, "blocks": args.blocks,
                    "deadline_ms": deadline * 1000, "worker_processing": stats(processing),
                    "client_round_trip": stats(round_trip), "peak": peak,
                    "worker_deadline_misses": sum(v > deadline for v in processing),
                    "round_trip_deadline_misses": sum(v > deadline for v in round_trip)})
        report["cleanup_completed"] = True
    except Exception as exc:
        report["error"] = str(exc)
        Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
        raise
    Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"report": args.output, "state_round_trip": True, "results": report["results"]}, indent=2))


if __name__ == "__main__":
    main()
