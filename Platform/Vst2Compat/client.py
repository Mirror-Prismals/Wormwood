"""Blocking control client for development tools, never for Cardinal's audio callback."""
import json
import os
from pathlib import Path
import select
import struct
import subprocess
import sys
import tempfile
import time
from protocol import VERSION, MAX_JSON, MAX_AUDIO, encode, reject_constant


class WorkerError(RuntimeError):
    pass


class Client:
    def __init__(self, python=sys.executable, worker=None):
        self.log = tempfile.TemporaryFile()
        self.child = subprocess.Popen([python, str(worker or Path(__file__).with_name("worker.py"))],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
                                      bufsize=0, start_new_session=True)
        os.set_blocking(self.child.stdin.fileno(), False)
        os.set_blocking(self.child.stdout.fileno(), False)
        self.sequence = 0

    def transfer(self, data, size, deadline):
        writing = data is not None
        fd = (self.child.stdin if writing else self.child.stdout).fileno()
        result = bytearray()
        offset = 0
        while offset < size:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise WorkerError("VST2 worker request timed out")
            readable, writable, _ = select.select([] if writing else [fd], [fd] if writing else [], [], remaining)
            if not (readable or writable):
                raise WorkerError("VST2 worker request timed out")
            try:
                if writing:
                    count = os.write(fd, memoryview(data)[offset:])
                else:
                    chunk = os.read(fd, size - offset)
                    count = len(chunk)
                    result.extend(chunk)
            except BlockingIOError:
                continue
            if not count:
                raise WorkerError("VST2 worker exited or closed its channel")
            offset += count
        return bytes(result)

    def request(self, op, audio=b"", timeout=5, **fields):
        if not 0 < timeout <= 120:
            raise ValueError("Worker timeout must be 0..120 seconds")
        self.sequence += 1
        packet = encode(dict(fields, version=VERSION, id=self.sequence, op=op), audio)
        deadline = time.monotonic() + timeout
        try:
            self.transfer(packet, len(packet), deadline)
            sizes = self.transfer(None, 8, deadline)
            control_size, audio_size = struct.unpack("<II", sizes)
            if not 1 <= control_size <= MAX_JSON or audio_size > MAX_AUDIO:
                raise WorkerError("Invalid worker response size")
            message = json.loads(self.transfer(None, control_size, deadline), parse_constant=reject_constant)
            payload = self.transfer(None, audio_size, deadline)
            if message.get("version") != VERSION or message.get("id") != self.sequence:
                raise WorkerError("Worker response identity mismatch")
            if message.get("ok") is not True:
                raise WorkerError(message.get("error", "Worker failed"))
            return message["result"], payload
        except Exception:
            self.abort()
            raise

    def abort(self):
        if self.child.poll() is None:
            self.child.kill()
        self.child.wait(timeout=5)
        self.child.stdin.close()
        self.child.stdout.close()
        self.log.close()

    def close(self):
        try:
            if self.child.poll() is None:
                self.request("close")
                self.child.wait(timeout=5)
        finally:
            self.abort()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
