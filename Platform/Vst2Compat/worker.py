#!/usr/bin/env python3
"""One plugin per process, serialized control/audio messages; no audio device or UI."""
import os
import sys
import time
from protocol import VERSION, receive, send
from session import Session


def main():
    # Keep native framework logging away from the binary protocol channel.
    output = os.fdopen(os.dup(sys.stdout.fileno()), "wb", buffering=0)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    session = None
    request_id = None
    metadata_cursor = 0
    metadata_remaining = 0
    try:
        while True:
            request, audio = receive(sys.stdin.buffer)
            request_id = request.get("id")
            if request.get("version") != VERSION or type(request_id) is not int:
                raise ValueError("Invalid worker protocol version/request id")
            op = request.get("op")
            if op != "process" and audio:
                raise ValueError("Only process requests accept audio payloads")
            result, output_audio = {}, b""
            if op == "open":
                if session is not None:
                    raise ValueError("Worker already owns a plugin")
                start = time.perf_counter()
                architecture = request.get('architecture', 'x86_64')
                if architecture == 'auto':
                    from architecture import detect_architecture
                    architecture = detect_architecture(request['plugin'])
                if architecture == 'x86_64':
                    request = dict(request, libraries=request.get('libraries', []) +
                                   request.get('optional_x86_64_libraries', []))
                if architecture == 'i386':
                    from session32 import Session32
                    session = Session32(request)
                elif architecture == 'x86_64':
                    session = Session(request)
                else:
                    raise ValueError('Unsupported VST2 guest architecture')
                result = session.describe()
                result["startup_seconds"] = time.perf_counter() - start
            elif session is None:
                raise ValueError("Worker must be opened first")
            elif op == "parameters":
                result["parameters"] = session.parameter_info()
            elif op == "set_parameter":
                session.runtime.set_parameter(request["index"], request["value"])
                result["value"] = session.runtime.get_parameter(request["index"])
            elif op == "save_state":
                result["state"] = session.save_state()
            elif op == "load_state":
                result["parameters"] = session.load_state(request["state"])
            elif op == "process":
                start = time.perf_counter()
                output_audio = session.process(request, audio)
                result.update(process_seconds=time.perf_counter() - start,
                              frames=request["frames"], channels=session.runtime.outputs)
                if request.get('parameter_metadata'):
                    if request.get('parameters'):
                        metadata_cursor, metadata_remaining = 0, session.runtime.params
                    # Refresh dependent names/displays without re-reading every
                    # parameter on one audio request. No extra IPC round trip.
                    if metadata_remaining:
                        result['parameter_info'] = session.parameter_info([metadata_cursor])
                        metadata_cursor += 1
                        metadata_remaining -= 1
            elif op == "close":
                session.close()
                session = None
            else:
                raise ValueError("Unknown worker operation")
            send(output, {"version": VERSION, "id": request_id, "ok": True, "result": result}, output_audio)
            if op == "close":
                return 0
    except EOFError:
        return 0  # Parent disappeared; finally still releases native resources.
    except Exception as exc:
        error = {"version": VERSION, "id": request_id, "ok": False, "error": str(exc)}
        if session:
            error["diagnostics"] = session.runtime.diagnostics()
        try:
            send(output, error)
        except (OSError, ValueError, EOFError):
            pass
        return 1  # Do not reuse a partially mutated/faulted plugin.
    finally:
        if session:
            session.runtime.close(suppress=True)
        output.close()


if __name__ == "__main__":
    raise SystemExit(main())
