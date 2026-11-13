#!/usr/bin/env python3
"""Isolated original-editor worker. No audio processing or native desktop UI."""
import os
import sys
from editor_session import EditorSession
from protocol import VERSION, receive, send


def main():
    output = os.fdopen(os.dup(sys.stdout.fileno()), 'wb', buffering=0)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    editor, request_id = None, None
    try:
        while True:
            request, payload = receive(sys.stdin.buffer)
            request_id = request.get('id')
            if request.get('version') != VERSION or type(request_id) is not int or payload:
                raise ValueError('Invalid editor protocol request')
            op = request.get('op')
            result, frame = {}, b''
            if op == 'open':
                if editor is not None:
                    raise ValueError('Editor already open')
                architecture = request.get('architecture', 'x86_64')
                if architecture == 'auto':
                    from architecture import detect_architecture
                    architecture = detect_architecture(request['plugin'])
                if architecture == 'i386':
                    from editor_session32 import EditorSession32
                    editor = EditorSession32(request)
                elif architecture == 'x86_64':
                    request = dict(request, libraries=request.get('libraries', []) +
                                   request.get('optional_x86_64_libraries', []))
                    editor = EditorSession(request)
                else:
                    raise ValueError('Unsupported VST2 editor architecture')
                result, frame = editor.frame()
                result.update(binary_sha256=editor.session.identity, parameters=editor.values(),
                              generic_editor_fallback=True, separate_audio_worker=True)
            elif editor is None:
                raise ValueError('Editor must be opened first')
            elif op == 'frame':
                editor.apply_parameters(request.get('parameters', []))
                result, frame = editor.frame()
            elif op in ('click', 'drag'):
                editor.apply_parameters(request.get('parameters', []))
                result, frame = editor.gesture(op, request.get('coordinates'))
            elif op == 'menu':
                result, frame = editor.select_menu(request.get('selection'))
            elif op == 'mouse':
                editor.apply_parameters(request.get('parameters', []))
                result, frame = editor.mouse(request.get('phase'), request.get('coordinates'))
            elif op == 'close':
                editor.close()
                editor = None
            else:
                raise ValueError('Unsupported editor operation; audio processing is prohibited')
            send(output, {'version': VERSION, 'id': request_id, 'ok': True, 'result': result}, frame)
            if op == 'close':
                return 0
    except EOFError:
        return 0
    except Exception as exc:
        error = {'version': VERSION, 'id': request_id, 'ok': False, 'error': str(exc),
                 'generic_editor_fallback': True}
        if editor:
            editor.failed = True
            tracking = getattr(editor, 'tracking', None)
            if editor.session is not None and (tracking is None or not tracking.thread.is_alive()):
                error['diagnostics'] = editor.runtime.diagnostics()
        try:
            send(output, error)
        except (ValueError, OSError, EOFError):
            pass
        return 1
    finally:
        try:
            if editor:
                editor.close()
        finally:
            output.close()


if __name__ == '__main__':
    raise SystemExit(main())
