"""Finite source/asset checks, native captures and local Mac packaging.

Run through Codex Process Jobs. Visual inspection of the captures remains a
separate required step. Does not build/upload an iOS application.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "BuildSupport"))
from checked_process import run_checked

OUT = ROOT / "output/checks" / ("input-monitor-" + datetime.datetime.now().strftime("%Y%m%d-%H%M%S"))
OUT.mkdir(parents=True, exist_ok=False)
report = {"output": str(OUT), "steps": [], "visual_review": "pending", "uploaded": False,
          "phone_verified": False, "checks_completed": False}


def save():
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")


def run(name, command, timeout=300, env=None):
    print(name, flush=True)
    started = time.monotonic()
    status = 0
    try:
        run_checked([str(v) for v in command], cwd=ROOT, log=OUT / (name + ".log"), timeout=timeout, env=env)
    except subprocess.CalledProcessError as error:
        status = error.returncode
    except subprocess.TimeoutExpired:
        status = 124
    report["steps"].append({"name": name, "status": status, "seconds": round(time.monotonic()-started, 3)})
    save()
    if status:
        print((OUT / (name + ".log")).read_text(errors="replace")[-7000:], flush=True)
        raise SystemExit(status)


def fingerprints():
    paths = ["Workstation/InputMonitor.rs", "Workstation/PanelLayout.rs", "Workstation/PanelMotion.rs",
             "Workstation/DecorativeControls.rs", "Workstation/mod.rs",
             "Workstation/Arrangement.rs", "Workstation/PianoRollScene.rs", "Workstation/TimelineScene.rs",
             "Workstation/TimelineKernel.glsl", "Workstation/TimelineEngravingKernel.glsl",
             "BaseSystem/LayerFive/AudioWorkstationSystem.rs",
             "Workstation/UserStyle.rs", "Workstation/UserStyleKernel.glsl", "Workstation/StyleShader.rs",
             "BaseSystem/LayerFour/WorkstationPanelSystem.rs", "Render/Workstation.rs",
             "Systems/WorkstationPanelSystem.json", "Systems/AudioWorkstationSystem.json",
             "Procedures/systems.json", "BuildSupport/macOS/build_app.py",
             "BuildSupport/Blender/export_input_monitor.py", "BuildSupport/Blender/validate_input_monitor.py"]
    files = [ROOT / p for p in paths] + list((ROOT / "Procedures/Shaders").glob("workstation*"))
    files += list((ROOT / "Art/AudioWorkstation/InputMonitor").glob("InputMonitor_*.glb"))
    files += [ROOT / "Art/AudioWorkstation/InputMonitor" / p for p in ["runtime.json", "runtime.bin"]]
    # A concurrent DAW addition shares the immutable packet and bottom group.
    for relative in ["Workstation/EdisonDemo.rs", "Art/AudioWorkstation/EdisonHardware/runtime.json",
                     "Art/AudioWorkstation/EdisonHardware/runtime.bin", "Workstation/BubbleNotes.rs",
                     "Workstation/BubbleNotesKernel.glsl", "BuildSupport/Blender/export_bubble_notes.py",
                     "Art/AudioWorkstation/BubbleNotes/BubbleNotes.blend",
                     "Art/AudioWorkstation/BubbleNotes/BubbleNotes_runtime.json",
                     "Art/AudioWorkstation/BubbleNotes/BubbleNotes_runtime.bin",
                     "Workstation/ControlEngraving.rs", "Workstation/ControlEngravingKernel.glsl",
                     "BuildSupport/Blender/make_control_engraving.py",
                     "BuildSupport/Blender/export_workstation_controls.py",
                     "Art/AudioWorkstation/ControlEngraving/control_engraving.json",
                     "Art/AudioWorkstation/ControlEngraving/control_engraving.bin",
                     "Art/AudioWorkstation/DecorativeControls/controls.json",
                     "Art/AudioWorkstation/DecorativeControls/controls.bin",
                     "Art/AudioWorkstation/Timeline/runtime.json", "Art/AudioWorkstation/Timeline/runtime.bin",
                     "Art/AudioWorkstation/EdisonHardware/connected.json", "Art/AudioWorkstation/EdisonHardware/connected.bin",
                     "BuildSupport/Blender/export_timeline.py", "BuildSupport/Blender/export_edison_demo.py"]:
        if (ROOT / relative).is_file():
            files.append(ROOT / relative)
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}


def main():
    if sys.argv[1:] == ["--existing-assets"]:
        art = ROOT / "Art/AudioWorkstation/InputMonitor"
        manifest = json.loads((art / "runtime.json").read_text())
        assert set(manifest["source_sha256"]) == {"InputMonitor_Off.glb", "InputMonitor_Auto.glb", "InputMonitor_In.glb"}
        for name, digest in manifest["source_sha256"].items():
            assert hashlib.sha256((art / name).read_bytes()).hexdigest() == digest
        assert hashlib.sha256((art / "runtime.bin").read_bytes()).hexdigest() == manifest["runtime_sha256"]
        report["existing_asset_hashes_verified"] = True
        save()
    elif not sys.argv[1:]:
        run("export-input-monitor", [sys.executable, "BuildSupport/Blender/export_input_monitor.py"], 240)
    else:
        raise ValueError("Expected no arguments or --existing-assets")
    # The current shared scene loads the bubble packet even with no piano roll
    # open. Prepare that required generated dependency when it is absent; use
    # its existing exporter without modifying or saving the authored .blend.
    bubble_art = ROOT / "Art/AudioWorkstation/BubbleNotes"
    if (ROOT / "Workstation/BubbleNotes.rs").is_file():
        if any(not (bubble_art / ("BubbleNotes_runtime." + suffix)).is_file()
               for suffix in ["json", "bin"]):
            run("prepare-required-bubble-packet", [
                "/Applications/Blender.app/Contents/MacOS/Blender", "--background",
                bubble_art / "BubbleNotes.blend", "--python-exit-code", "1", "--python",
                ROOT / "BuildSupport/Blender/export_bubble_notes.py"], 240)
        bubble_manifest = json.loads((bubble_art / "BubbleNotes_runtime.json").read_text())
        assert hashlib.sha256((bubble_art / "BubbleNotes.blend").read_bytes()).hexdigest() == bubble_manifest["source_sha256"]
    if (ROOT / "Workstation/ControlEngraving.rs").is_file():
        engraving_art = ROOT / "Art/AudioWorkstation/ControlEngraving"
        if any(not (engraving_art / ("control_engraving." + suffix)).is_file()
               for suffix in ["json", "bin"]):
            run("prepare-required-control-engraving", [
                "/Applications/Blender.app/Contents/MacOS/Blender", "--background", "--factory-startup",
                "--python-exit-code", "1", "--python",
                ROOT / "BuildSupport/Blender/make_control_engraving.py"], 480)
        manifest_hash = hashlib.sha256((engraving_art / "control_engraving.json").read_bytes()).hexdigest()
        controls = json.loads((ROOT / "Art/AudioWorkstation/DecorativeControls/controls.json").read_text())
        if controls.get("control_engraving_manifest_sha256") != manifest_hash:
            run("prepare-matching-controls-packet", [sys.executable, "BuildSupport/Blender/export_workstation_controls.py"], 240)
    # Other DAW work shares this checkout. Catch incomplete module declarations
    # before spending a build on a source file that has not been written yet.
    modules = list((ROOT / "Workstation").glob("*.rs")) + [ROOT / p for p in
        ["BaseSystem/LayerFour/mod.rs", "BaseSystem/LayerFive/mod.rs"]]
    for module in modules:
        for name in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', module.read_text()):
            assert (module.parent / name).is_file(), f"shared scene source is incomplete: {module.parent / name}"
    report["source_sha256"] = fingerprints()
    # This also builds the native binary and its Vulkan SPIR-V frontends.
    run("native-build-and-intent", ["cargo", "run", "--offline", "--locked", "--release", "--", "--inspect-intent", "--json"], 900)
    run("intent-json", [ROOT / "target/release/wormwood", "--inspect-intent", "--json"], 30)
    intent = json.loads((OUT / "intent-json.log").read_text())
    report["intent_summary"] = intent["summary"]
    assert intent["summary"]["active_structural_mismatches"] == 0
    # Desktop GL compiles at rendering; verify the generated GLES syntax too.
    for kind in ["vert", "frag"]:
        run("gles-" + kind, ["glslangValidator", "-S", kind, "Procedures/Shaders/workstation.ios." + kind], 60)
    for backend in ["WebGPU", "Vulkan", "OpenGL"]:
        destination = OUT / backend
        destination.mkdir()
        env = os.environ.copy()
        env.pop("WORMWOOD_STYLE", None)
        env.update(WORMWOOD_ASSET_ROOT=str(ROOT), WORMWOOD_OUTPUT_ROOT=str(destination),
                   WORMWOOD_FULLSCREEN="1", WORMWOOD_WORKSTATION_RETINA="0")
        # The explicit core arrangement fixture supplies a MIDI lane and a CC
        # lane. Normal launch remains an empty user document.
        run("render-" + backend, [ROOT / "target/release/wormwood", "--check-arrangement", backend], 240, env)
        packet = json.loads((destination / "workstation-optical-frame.json").read_text())
        rows = packet["rows"]
        assert len(rows) == 49408 and rows[12][0] >= 13, "dynamic control packet disagrees"
        pose_base, count = map(int, rows[247][:2])
        assert count == 1, "MIDI lane needs one light; CC must have none"
        assert rows[pose_base+1][3] == 1272286, "visual lanes start Off/red"
        assert all(rows[226+i][3] == 0 for i in range(6)), "legacy preview leaked into live tracks"
        width, height = packet["logical_viewport"]
        mpp = 1.3 / height
        # Read TimelineScene's independent body/clip bounds, including its
        # right57px monitor reserve. Compare actual native monitor poses.
        column = rows[249][2] / mpp + width / 2
        top = height / 2 - rows[249][3] / mpp
        bottom = height / 2 - rows[249][1] / mpp
        s = 60. / 358.
        poses = []
        for index in range(count):
            pose = rows[pose_base + 2*index]
            x = pose[0] / mpp + width / 2
            y = height / 2 - pose[1] / mpp
            assert abs(x - (column + 28.5 + 10*s)) < .001, "not beside the actual timeline"
            assert abs(y - (top + 32 + 64*index + 4*s)) < .001, "not aligned with the actual lane"
            assert abs(pose[3] - s*mpp/.0005) < 1e-6, "bottom mixer altered monitor scale"
            poses.append([x, y])
        assert abs((height/2 - rows[239][3]/mpp) - top) < .001
        assert abs((height/2 - rows[238][3]/mpp) - bottom) < .001
        report.setdefault("timeline_placement", {})[backend] = {
            "poses_px": poses, "clip_y_px": [top, bottom],
            "source": "actual arrangement MIDI lane; CC omitted, visual-only Off state"}
        session = json.loads((destination / ("arrangement-" + backend + ".json")).read_text())
        assert session["end_reason"] == "frame_limit" and session["entities_remaining"] == 0
        report.setdefault("dimensions", {})[backend] = packet["logical_viewport"]
        report.setdefault("captures", {})[backend] = []
        for path in destination.glob("*.ppm"):
            png = path.with_suffix(".png")
            subprocess.run(["/usr/bin/sips", "-s", "format", "png", str(path), "--out", str(png)],
                           check=True, stdout=subprocess.DEVNULL)
            report["captures"][backend].append(str(png))
        assert report["captures"][backend], "native capture missing"
        save()
    # Keep native placement evidence even if another newly added integration
    # test fails. A package still requires this complete focused suite to pass.
    run("workstation-tests", ["cargo", "test", "--offline", "--locked", "--release", "--lib", "workstation_", "--", "--nocapture"], 900)
    for module in ["arrangement", "timeline_scene"]:
        run(module + "-tests", ["cargo", "test", "--offline", "--locked", "--release", "--lib",
                               "workstation::" + module + "::", "--", "--nocapture"], 900)
    assert fingerprints() == report["source_sha256"], "relevant sources changed during validation"
    run("package-mac", [sys.executable, "BuildSupport/macOS/build_app.py"], 900)
    app = ROOT / "output/macOS/MirrorPrismals.app"
    packaged = app / "Contents/Resources/EngineAssets/Art/AudioWorkstation/InputMonitor"
    for name in ["runtime.json", "runtime.bin"]:
        assert (packaged / name).read_bytes() == (ROOT / "Art/AudioWorkstation/InputMonitor" / name).read_bytes()
    run("verify-mac-signature", ["codesign", "--verify", "--deep", "--strict", app], 30)
    report["app"] = str(app)
    report["checks_completed"] = True
    save()
    print(json.dumps({"report": str(OUT / "report.json"), "checks_completed": True,
                      "visual_review": "pending", "app": str(app)}), flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        report["error"] = str(error)
        save()
        raise
