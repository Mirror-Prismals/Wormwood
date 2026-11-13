#!/usr/bin/env python3
"""Freeze, build and capture a bounded four-dimensional workstation state family.

Stages are explicit and resumable, for example:
  python3 BuildSupport/Workstation/capture_state_family.py --output output/checks/state-family --prepare
  python3 BuildSupport/Workstation/capture_state_family.py --output output/checks/state-family --build --capture

Each build/capture stage runs in the foreground with a finite timeout and log.
Only the private evidence/source AudioWorkstationSystem.rs is patched. Geometry,
shaders, lighting, optical budgets and native resolution remain unchanged.
"""
from pathlib import Path
import argparse
import hashlib
import itertools
import json
import os
import shutil
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "BuildSupport"))
from checked_process import run_checked

VERSION = 1
PATCH_FILE = "BaseSystem/LayerFive/AudioWorkstationSystem.rs"
EXCLUDED = {".git", "target", "output", "__pycache__", ".DS_Store", ".venv", "venv", "node_modules"}
HEADER = b"P6\n640 480\n255\n"


def digest(path):
    hasher = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def write_json(path, value):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def files(root):
    result = []
    for directory, folders, names in os.walk(root, followlinks=False):
        folders[:] = sorted(name for name in folders if name not in EXCLUDED)
        for name in sorted(names):
            if name in EXCLUDED or name.endswith((".pyc", ".pyo")):
                continue
            path = Path(directory) / name
            if path.is_file():
                result.append(path)
        # Frozen sources must not silently depend on mutable external folders.
        for name in folders:
            if (Path(directory) / name).is_symlink():
                raise RuntimeError(f"Directory symlink needs an explicit frozen source: {Path(directory) / name}")
    return result


def hashes(root):
    return {str(path.relative_to(root)): digest(path) for path in files(root)}


def replace_once(text, before, after):
    if text.count(before) != 1:
        raise RuntimeError(f"Private patch anchor expected once: {before[:100]!r}")
    return text.replace(before, after, 1)


def patched_source(original, template):
    text = replace_once(original,
        '    let dt = input["dt"]\n',
        '    if !input["state_family"].is_null() {\n'
        '        return state_family_step(state, host, &input["state_family"]);\n'
        '    }\n    let dt = input["dt"]\n')
    text = replace_once(text,
        '    // A rejected Begin belongs to another live scene and must not close it.\n',
        '    let state_family = state_family_request(input, embedded, limit, benchmark.is_some())?;\n'
        '    // A rejected Begin belongs to another live scene and must not close it.\n')
    text = replace_once(text,
        '    let execution = (|| -> Result<&\'static str, String> {\n',
        '    let execution = (|| -> Result<&\'static str, String> {\n'
        '        if let Some(family) = state_family.as_ref() {\n'
        '            state_family_capture(host, family, &visual, &mut frames)?;\n'
        '            return Ok("frame_limit");\n'
        '        }\n')
    return text + "\n" + template


def state_family():
    states = []
    for split, knots in [("train", [0.0, 0.5, 1.0]), ("heldout", [0.25, 0.75])]:
        for index, extensions in enumerate(itertools.product(knots, repeat=4)):
            states.append({"id": f"{split}-{index:03d}", "split": split, "extensions": list(extensions)})
    by_id = {state["id"]: state for state in states}
    repeats = []
    for name in ["train-000", "train-040", "train-080", "heldout-000", "heldout-015"]:
        repeats.append({**by_id[name], "id": "repeat-" + name, "split": "repeat", "reference": name})
    return {"schema": 1, "axes": ["top", "bottom", "left", "right"], "states": states, "repeats": repeats,
        "fixed": {"opening_order": [1, 2, 3, 4], "open": [True] * 4, "left_width_scale": 1.0,
            "bottom_height_scale": 1.0, "overview_hide": [0.0] * 4, "canvas_scale": 1.0,
            "swap_sides": False, "swap_horizontal": False, "dimensions": [640, 480],
            "framebuffer": [640, 480], "camera": "existing orthographic", "style": "built-in",
            "document": "fresh empty arrangement", "plugins": "fresh process presentation",
            "playhead_seconds": 0.0, "bubbles": None, "spheres": None},
        "coverage": "Four independent continuous panel-extension axes at one fixed viewport, opening order and empty document; not arbitrary content/style/layout coverage."}


def verify_snapshot(output):
    manifest = json.loads((output / "snapshot.json").read_text())
    if manifest["version"] != VERSION:
        raise RuntimeError("Snapshot was prepared by another harness version")
    actual = hashes(output / "source")
    expected = manifest["frozen_sha256"]
    changed = [name for name in actual.keys() | expected.keys() if actual.get(name) != expected.get(name)]
    if changed:
        raise RuntimeError(f"Frozen source changed: {changed[:20]}")
    return manifest


def prepare(args):
    output = args.output
    if (output / "snapshot.json").exists():
        verify_snapshot(output)
        print("prepare: existing frozen snapshot verified", flush=True)
        return
    if (output / "source").exists():
        raise RuntimeError("Unmanifested source directory exists; choose a fresh output directory")
    template = Path(__file__).with_name("state_family_capture.rs").read_text()
    original = (args.source_root / PATCH_FILE).read_text()
    patched = patched_source(original, template)
    temporary = output / ("source-preparing-" + uuid.uuid4().hex)
    try:
        shutil.copytree(args.source_root, temporary,
            ignore=shutil.ignore_patterns(*EXCLUDED, "*.pyc", "*.pyo"), symlinks=False)
        copied = hashes(temporary)
        current = hashes(args.source_root)
        if copied != current or (temporary / PATCH_FILE).read_text() != original:
            changed = [name for name in copied.keys() | current.keys() if copied.get(name) != current.get(name)]
            raise RuntimeError(f"Live sources changed during snapshot preparation: {changed[:20]}")
        (temporary / PATCH_FILE).write_text(patched)
        frozen = hashes(temporary)
        changed = [name for name in copied if copied[name] != frozen[name]]
        if changed != [PATCH_FILE]:
            raise RuntimeError(f"Private patch changed unexpected files: {changed}")
        temporary.rename(output / "source")
        write_json(output / "snapshot.json", {"version": VERSION, "source_root": str(args.source_root),
            "original_sha256": copied, "frozen_sha256": frozen, "patched_only": [PATCH_FILE],
            "template_sha256": hashlib.sha256(template.encode()).hexdigest(), "art_and_shaders_unchanged": True})
        write_json(output / "states.json", state_family())
        print(f"prepare: froze {len(frozen)} files; only private {PATCH_FILE} patched", flush=True)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def build(args):
    verify_snapshot(args.output)
    binary = args.output / "target/release/wormwood"
    record = args.output / "build.json"
    if record.exists() and binary.exists():
        previous = json.loads(record.read_text())
        if previous["binary_sha256"] == digest(binary):
            print("build: existing frozen executable verified", flush=True)
            return
        raise RuntimeError("Recorded executable changed; choose a fresh output directory")
    started = time.monotonic()
    run_checked(["cargo", "build", "--offline", "--locked", "--release", "--bin", "wormwood",
        "--target-dir", str(args.output / "target")], cwd=args.output / "source",
        log=args.output / "build.log", timeout=args.build_timeout)
    verify_snapshot(args.output)
    write_json(record, {"binary_sha256": digest(binary), "seconds": time.monotonic() - started,
        "command": "cargo build --offline --locked --release --bin wormwood --target-dir <evidence>/target"})


def captured_entry(output, state):
    image = output / "captures" / (state["id"] + ".ppm")
    optical = image.with_suffix(".json")
    if not image.exists() or not optical.exists():
        return None
    pixels = image.read_bytes()
    if not pixels.startswith(HEADER) or len(pixels) != len(HEADER) + 640 * 480 * 3:
        raise RuntimeError(f"Invalid capture dimensions: {image}")
    packet = json.loads(optical.read_text())
    if packet["logical_viewport"] != [640, 480] or packet["rows"][8][:2] != [640, 480]:
        raise RuntimeError(f"Invalid optical sampling dimensions: {optical}")
    return {"state": state, "image": str(image.relative_to(output)), "image_sha256": digest(image),
        "optical": str(optical.relative_to(output)), "optical_sha256": digest(optical),
        "dimensions": [640, 480], "optical_rows": len(packet["rows"])}


def capture(args):
    verify_snapshot(args.output)
    build_record = json.loads((args.output / "build.json").read_text())
    binary = args.output / "target/release/wormwood"
    if digest(binary) != build_record["binary_sha256"]:
        raise RuntimeError("Capture executable does not match validated build")
    family = json.loads((args.output / "states.json").read_text())
    if family != state_family():
        raise RuntimeError("State-family definition changed since preparation")
    recorded = {}
    for path in sorted(args.output.glob("state-family-*.json")):
        run = json.loads(path.read_text())
        for entry in run.get("captures", []):
            recorded[entry["state"]["id"]] = entry["state"]
    previous_path = args.output / "capture-manifest.json"
    previous = json.loads(previous_path.read_text()) if previous_path.exists() else None
    if previous:
        for entry in previous["captures"]:
            actual = captured_entry(args.output, entry["state"])
            if actual != entry:
                raise RuntimeError(f"Previously recorded capture changed: {entry['state']['id']}")
    env = os.environ.copy()
    for key in ["WORMWOOD_STYLE", "WORMWOOD_STOP_FILE", "WORMWOOD_WORKSTATION_CAPTURE_DT", "WORMWOOD_STATE_FAMILY"]:
        env.pop(key, None)
    env.update(WORMWOOD_ASSET_ROOT=str(args.output / "source"), WORMWOOD_OUTPUT_ROOT=str(args.output),
        WORMWOOD_FULLSCREEN="0", WORMWOOD_WORKSTATION_RETINA="0", WORMWOOD_PERFORMANCE="0",
        WORMWOOD_WORKSTATION_GPU_TIMING="0")
    for label, group in [("lattice", family["states"]), ("repeats", family["repeats"])]:
        pending = []
        for state in group:
            entry = captured_entry(args.output, state)
            if entry is None or recorded.get(state["id"]) != state:
                pending.append(state)
        if not pending:
            print(f"capture: {label} already complete", flush=True)
            continue
        run = label + "-" + uuid.uuid4().hex[:12]
        env["WORMWOOD_STATE_FAMILY"] = json.dumps({"run": run, "states": pending}, separators=(",", ":"))
        print(f"capture: {label} rendering {len(pending)} exact states", flush=True)
        try:
            run_checked([str(binary), "--workstation", "WebGPU", "--frames", str(len(pending))],
                cwd=args.output / "source", log=args.output / (run + ".log"), timeout=args.capture_timeout, env=env)
        finally:
            verify_snapshot(args.output)
        report = json.loads((args.output / ("state-family-" + run + ".json")).read_text())
        if report["complete"] is not True or len(report["captures"]) != len(pending):
            raise RuntimeError("Incomplete native family capture")
        session = json.loads((args.output / "workstation-WebGPU.json").read_text())
        if session["frames"] != len(pending) or session["end_reason"] != "frame_limit" or session["entities_remaining"] != 0:
            raise RuntimeError("Native capture session did not close cleanly")
    entries = [captured_entry(args.output, state) for state in family["states"] + family["repeats"]]
    if any(entry is None for entry in entries):
        raise RuntimeError("Missing reference images after capture")
    lookup = {entry["state"]["id"]: entry for entry in entries}
    repeat_checks = []
    for state in family["repeats"]:
        repeat, reference = lookup[state["id"]], lookup[state["reference"]]
        repeat_checks.append({"repeat": state["id"], "reference": state["reference"],
            "image_identical": repeat["image_sha256"] == reference["image_sha256"],
            "optical_identical": repeat["optical_sha256"] == reference["optical_sha256"]})
    deterministic = all(check["image_identical"] and check["optical_identical"] for check in repeat_checks)
    manifest = {"schema": 1, "complete": True, "backend": "WebGPU", "family": family,
        "binary_sha256": build_record["binary_sha256"], "captures": entries,
        "repeat_checks": repeat_checks, "determinism_passed": deterministic,
        "art_and_shaders_unchanged": True, "production_files_patched": False,
        "image_format": "Canonical native PPM RGB8 after existing AgX display transform"}
    write_json(previous_path, manifest)
    normalized = {key: value for key, value in manifest.items() if key != "captures"}
    normalized.update(width=640, height=480, states=[{
        "id": state["id"], "split": "holdout" if state["split"] == "heldout" else state["split"],
        "extensions": state["extensions"], "image": state["id"] + ".ppm",
        "image_sha256": lookup[state["id"]]["image_sha256"],
        "optical": state["id"] + ".json", "optical_sha256": lookup[state["id"]]["optical_sha256"],
    } for state in family["states"]])
    write_json(args.output / "captures" / "manifest.json", normalized)
    print(json.dumps({"manifest": str(previous_path), "captures": len(entries), "determinism_passed": deterministic}), flush=True)
    if not deterministic:
        raise RuntimeError("Repeated canonical states differ; compression/reference use is not validated")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--build", action="store_true")
    parser.add_argument("--capture", action="store_true")
    parser.add_argument("--build-timeout", type=int, default=600)
    parser.add_argument("--capture-timeout", type=int, default=360)
    args = parser.parse_args()
    if not (args.prepare or args.build or args.capture):
        parser.error("select --prepare, --build and/or --capture")
    args.output = args.output.resolve()
    args.source_root = args.source_root.resolve()
    if args.output == args.source_root or args.output in args.source_root.parents:
        parser.error("output must be separate from and not contain the source root")
    # The default output subtree is excluded from the full source snapshot.
    if args.source_root in args.output.parents and args.output.relative_to(args.source_root).parts[0] not in EXCLUDED:
        parser.error("an output inside the source root must be under output/ or another excluded directory")
    if not (1 <= args.build_timeout <= 1800 and 1 <= args.capture_timeout <= 600):
        parser.error("timeouts exceed the bounded diagnostic limits")
    args.output.mkdir(parents=True, exist_ok=True)
    if args.prepare:
        prepare(args)
    if args.build:
        build(args)
    if args.capture:
        capture(args)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"state-family failed: {error}", file=sys.stderr, flush=True)
        raise SystemExit(1)
