#!/usr/bin/env python3
"""Headless editor lifecycle diagnostic; run only in a disposable process.

Never attach this probe to Cardinal's live audio worker. No native parent window
is supplied: successful dispatch is not evidence of a usable editor.
"""
import argparse
import json
from pathlib import Path
import struct
from macho import MachO
from runtime import Runtime, SCRATCH


def probe(args):
    report = {"native_editor_supported": False, "generic_editor_fallback": True,
              "live_audio_worker_touched": False, "parent_kind": args.parent + " diagnostic parent",
              "stages": []}
    runtime = None
    opened = False
    pool = None
    stage = "initialize"
    try:
        runtime = Runtime(MachO.from_path(args.plugin), trace_imports=True,
                          libraries=[MachO.from_path(p) for p in args.guest_library],
                          startup_seconds=args.startup_seconds, resource_root=args.resource_root, editor_views=args.parent == "offscreen")
        pool = runtime.call(runtime.shim_address("_objc_msgSend"), runtime.objc.class_address,
                            runtime.objc.registry.intern("new"))
        report["advertises_editor"] = bool(runtime.flags & 1)
        if not report["advertises_editor"]:
            report["outcome"] = "plugin does not advertise an editor"
            return report
        stage = "get_rectangle"
        runtime.scratch_next = SCRATCH
        pointer = runtime.scratch(bytes(8))
        result = runtime.dispatch(13, pointer=pointer)
        rectangle = runtime.read_u64(pointer)
        entry = {"stage": stage, "return_value": result}
        if rectangle:
            top, left, bottom, right = struct.unpack("<4h", runtime.read(rectangle, 8))
            entry["rectangle"] = {"top": top, "left": left, "bottom": bottom, "right": right}
            entry["valid_dimensions"] = 0 < right - left <= 16384 and 0 < bottom - top <= 16384
        report["stages"].append(entry)
        if args.attempt_open:
            stage = "open_offscreen" if args.parent == "offscreen" else "open_without_native_parent"
            # An intentionally unmapped guest token gets past null-parent checks
            # without pretending to implement a view or exposing a native pointer.
            # The first dereference or unsupported platform call must trap.
            parent = 0 if args.parent == "null" else 0x40000000000
            if args.parent == "offscreen":
                registry = runtime.objc.registry
                parent_class = registry.require(registry.builtin("NSView"))
                parent = registry.views.allocate(parent_class)
                registry.views.initialize(parent, (0., 0., 640., 480.))
                if args.click or args.drag:
                    registry.windows.attach(parent)
            result = runtime.dispatch(14, pointer=parent)
            report["stages"].append({"stage": stage, "return_value": result})
            opened = bool(result)
            if opened:
                stage = "idle"
                result = runtime.dispatch(19)
                report["stages"].append({"stage": stage, "return_value": result})
                if args.attempt_draw:
                    stage = "draw_callback"
                    views = runtime.objc.registry.views
                    candidates = [p for p, v in views.records.items()
                                  if v['parent'] == parent and runtime.objc.registry.inherited(
                                      runtime.objc.registry.instances[p][0], 'methods', 'drawRect:')]
                    if not candidates:
                        raise RuntimeError('No attached editor view advertises drawRect:')
                    for view in candidates:
                        report["stages"].append({"stage": stage, **views.probe_draw(view)})
                    for action, coordinates in (('click', args.click), ('drag', args.drag)):
                        if not coordinates:
                            continue
                        if len(candidates) != 1:
                            raise RuntimeError('Diagnostic mouse input requires exactly one editor view')
                        stage = 'mouse_' + action
                        before = [runtime.get_parameter(i) for i in range(runtime.params)]
                        getattr(views, 'probe_' + action)(candidates[0], *coordinates)
                        after = [runtime.get_parameter(i) for i in range(runtime.params)]
                        report['stages'].append({'stage': stage, 'coordinates': coordinates,
                            'changed_parameters': [{'index': i, 'before': a, 'after': b}
                                                   for i, (a, b) in enumerate(zip(before, after)) if a != b]})
                        stage = 'redraw_after_' + action
                        report['stages'].append({'stage': stage, **views.probe_draw(candidates[0])})
            report["outcome"] = "offscreen probe completed; interactive Cardinal integration remains unverified"
        else:
            report["outcome"] = "rectangle inspected; editor open not attempted"
    except Exception as exc:
        report.update(outcome="editor unavailable; retain generic editor", failed_stage=stage, error=str(exc))
        # After a trapped editor call, don't resume its partially mutated editor.
        opened = False
        if runtime is None and hasattr(exc, "details"):
            report["diagnostics"] = exc.details
    finally:
        if runtime is not None:
            report["diagnostics"] = runtime.diagnostics()
            if args.bitmap_output and runtime.objc.registry.graphics:
                graphics = runtime.objc.registry.graphics
                surfaces = list(graphics.surfaces)
                if len(surfaces) == 1:
                    try:
                        report['bitmap'] = graphics.export_png(surfaces[0], args.bitmap_output)
                        report['bitmap']['draw_completed'] = any(s.get('callback_completed') for s in report['stages'])
                    except Exception as exc:
                        report['bitmap_export_error'] = str(exc)
                else:
                    report['bitmap_export_note'] = f'Expected one editor bitmap, found {len(surfaces)}'
            if opened:
                try:
                    runtime.dispatch(15)
                    report["editor_closed"] = True
                    report["views_after_editor_close"] = runtime.objc.registry.views.diagnostics() if runtime.objc.registry.views else []
                except Exception as exc:
                    report["editor_close_error"] = str(exc)
                    report["editor_close_diagnostics"] = runtime.diagnostics()
            if pool is not None:
                try:
                    runtime.objc.destroy(pool)
                except Exception as exc:
                    report["pool_cleanup_error"] = str(exc)
            runtime.close(suppress=True)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plugin")
    parser.add_argument("--guest-library", action="append", default=[])
    parser.add_argument("--startup-seconds", type=float, default=15)
    parser.add_argument("--resource-root", help="Workspace mirror of absolute resource paths, used for read-only lookups")
    parser.add_argument("--attempt-open", action="store_true", help="Attempt editor open with a diagnostic parent; no actual window")
    parser.add_argument("--attempt-draw", action="store_true", help="Probe guest draw callbacks offscreen; graphics calls may trap, no rendered UI yet")
    parser.add_argument("--bitmap-output", help="Write the diagnostic RGBA bitmap as PNG, including partial output on a draw trap")
    parser.add_argument("--click", type=float, nargs=2, metavar=('X', 'Y'), help="Send a view-local diagnostic left click; never injects system events")
    parser.add_argument("--drag", type=float, nargs=4, metavar=('X', 'Y', 'END_X', 'END_Y'), help="Send a bounded view-local diagnostic mouse drag")
    parser.add_argument("--parent", choices=("null", "guarded", "offscreen"), default="null",
                        help="Use null, an unmapped token, or an opt-in offscreen NSView bridge; no visible window")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    if args.attempt_draw and (not args.attempt_open or args.parent != "offscreen"):
        parser.error('--attempt-draw requires --attempt-open --parent offscreen')
    if args.bitmap_output and not args.attempt_draw:
        parser.error('--bitmap-output requires --attempt-draw')
    if (args.click or args.drag) and not args.attempt_draw:
        parser.error('--click/--drag require --attempt-draw')
    report = probe(args)
    Path(args.output).write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if not k.endswith("diagnostics")}, indent=2))
    # An unsupported editor is an expected diagnostic outcome, not audio failure.
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
