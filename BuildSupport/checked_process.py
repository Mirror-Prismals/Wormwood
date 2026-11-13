"""Finite release steps with progress reporting and process-tree deadlines."""
from pathlib import Path
import os
import signal
import subprocess
import time


def run_checked(command, *, cwd, log, timeout, env=None, heartbeat=30):
    log = Path(log)
    started = time.monotonic()
    with log.open('w') as stream:
        process = subprocess.Popen(command, cwd=cwd, stdout=stream,
                                   stderr=subprocess.STDOUT, env=env,
                                   start_new_session=True)

        def stop_tree():
            # This group is created and owned by this exact Popen instance.
            # Include grandchildren such as Cargo's test executable.
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                pass
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()

        def interrupted(signum, _frame):
            raise SystemExit(128 + signum)

        handlers = {sig: signal.signal(sig, interrupted)
                    for sig in (signal.SIGTERM, signal.SIGINT)}
        try:
            while True:
                remaining = timeout - (time.monotonic() - started)
                if remaining <= 0:
                    print(f'{log.stem}: stopped after {timeout}s deadline', flush=True)
                    raise subprocess.TimeoutExpired(command, timeout)
                try:
                    status = process.wait(timeout=min(heartbeat, remaining))
                    break
                except subprocess.TimeoutExpired:
                    elapsed = time.monotonic() - started
                    size = log.stat().st_size
                    print(f'{log.stem}: running {elapsed:.0f}s; log {size} bytes; '
                          f'deadline {timeout}s', flush=True)
            if status:
                raise subprocess.CalledProcessError(status, command)
        finally:
            # Run on deadline, signal, or exceptional exit. An unsuccessful
            # step must never leave a test running while release continues.
            if process.poll() is None:
                stop_tree()
            for sig, handler in handlers.items():
                signal.signal(sig, handler)
