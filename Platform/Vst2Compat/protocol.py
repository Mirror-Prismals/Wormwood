"""Version 1 local pipe protocol. JSON control plus little-endian planar float32 audio."""
import json
import struct

VERSION = 1
MAX_JSON = 8 * 1024 * 1024
MAX_AUDIO = 1024 * 1024


def reject_constant(value):
    raise ValueError(f"Non-finite JSON value: {value}")


def encode(message, audio=b""):
    data = json.dumps(message, allow_nan=False, separators=(",", ":")).encode("utf-8")
    if len(data) > MAX_JSON or len(audio) > MAX_AUDIO:
        raise ValueError("Worker message exceeds protocol limit")
    return struct.pack("<II", len(data), len(audio)) + data + audio


def read_exact(stream, size):
    result = bytearray()
    while len(result) < size:
        data = stream.read(size - len(result))
        if not data:
            raise EOFError("Worker channel closed")
        result.extend(data)
    return bytes(result)


def receive(stream):
    json_size, audio_size = struct.unpack("<II", read_exact(stream, 8))
    if not 1 <= json_size <= MAX_JSON or audio_size > MAX_AUDIO:
        raise ValueError("Invalid worker message size")
    message = json.loads(read_exact(stream, json_size), parse_constant=reject_constant)
    if not isinstance(message, dict):
        raise ValueError("Worker control must be an object")
    return message, read_exact(stream, audio_size)


def send(stream, message, audio=b""):
    data = memoryview(encode(message, audio))
    while data:
        count = stream.write(data)
        if not count:
            raise EOFError("Worker output channel closed")
        data = data[count:]
