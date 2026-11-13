"""Serialized worker session: metadata, parameters, and portable project state."""
import base64
import hashlib
import math
import struct
from libc_shims import string_bytes
from macho import CompatibilityError, MachO
from runtime import Runtime, SCRATCH

MAX_STATE = 4 * 1024 * 1024


class Session:
    pointer_size = 8

    def __init__(self, request, *, editor_options=None):
        self.image = MachO.from_path(request["plugin"])
        self.identity = hashlib.sha256(self.image.data).hexdigest()
        self.runtime = Runtime(self.image, sample_rate=request.get("sample_rate", 48000),
                               block_size=request.get("block_size", 256),
                               trace_imports=request.get("experimental", False),
                               libraries=[MachO.from_path(p) for p in request.get("libraries", [])],
                               startup_seconds=request.get("startup_seconds", 15),
                               resource_root=request.get("resource_root"),
                               **(editor_options or {}))

    def parameter_info(self, indices=None):
        w = self.runtime
        result = []
        w.scratch_next = SCRATCH
        output = w.scratch(bytes(256))
        for index in range(w.params) if indices is None else indices:
            item = {"id": index, "value": w.get_parameter(index)}
            for opcode, field in ((8, "name"), (6, "units"), (7, "display")):
                w.cpu.mem_write(output, bytes(256))
                w.dispatch(opcode, index=index, pointer=output)
                item[field] = string_bytes(w, output, limit=256).decode("utf-8", errors="replace")
            result.append(item)
        return result

    def describe(self):
        w = self.runtime
        identity, version = struct.unpack("<ii", w.read(w.effect + 112, 8))
        return {"architecture": "x86_64", "binary_sha256": self.identity, "unique_id": identity, "plugin_version": version,
                "inputs": w.inputs, "outputs": w.outputs, "is_instrument": bool(w.flags & 256),
                "sample_rate": w.sample_rate, "block_size": w.block_size,
                "latency_frames": struct.unpack("<i", w.read(w.effect + 80, 4))[0],
                "parameters": self.parameter_info(), "programs": w.programs,
                "state_kind": "chunk" if w.flags & 32 else "parameters",
                "experimental": w.trace_imports, "unresolved_imports": w.missing_imports,
                "native_editor_supported": False, "rosetta_used": False}

    def state_buffer(self, data):
        self.runtime.scratch_next = SCRATCH
        return self.runtime.scratch(data)

    def state_pointer(self, address):
        return self.runtime.read_u64(address)

    def save_state(self):
        w = self.runtime
        state = {"version": 1, "binary_sha256": self.identity,
                 "kind": "chunk" if w.flags & 32 else "parameters"}
        if w.flags & 32:
            pointer = self.state_buffer(bytes(self.pointer_size))
            size = w.dispatch(23, index=0, pointer=pointer)  # bank, not just current program
            if not 0 < size <= MAX_STATE:
                raise CompatibilityError("Plugin returned an invalid/oversized state chunk")
            state["data"] = base64.b64encode(w.read(self.state_pointer(pointer), size)).decode("ascii")
        else:
            # Parameter-only plugins expose current-program state, not hidden DSP history.
            state["program"] = w.dispatch(3) if w.programs else None
            state["values"] = [w.get_parameter(i) for i in range(w.params)]
        return state

    def load_state(self, state, *, include_parameters=True):
        w = self.runtime
        if state.get("version") != 1 or state.get("binary_sha256") != self.identity:
            raise CompatibilityError("State version or plugin binary identity does not match")
        expected = "chunk" if w.flags & 32 else "parameters"
        if state.get("kind") != expected:
            raise CompatibilityError("State representation does not match plugin capabilities")
        if expected == "chunk":
            encoded = state.get("data", "")
            if not isinstance(encoded, str) or len(encoded) > (MAX_STATE + 2) // 3 * 4:
                raise CompatibilityError("State chunk exceeds limit")
            data = base64.b64decode(encoded, validate=True)
            if not data or len(data) > MAX_STATE:
                raise CompatibilityError("Empty or oversized state chunk")
            w.dispatch(24, index=0, value=len(data), pointer=self.state_buffer(data))
        else:
            values = state.get("values")
            program = state.get("program")
            if not isinstance(values, list) or len(values) != w.params or any(
                    type(v) not in (int, float) or not math.isfinite(v) or not 0 <= v <= 1 for v in values):
                raise CompatibilityError("Invalid parameter state")
            if w.programs and (type(program) is not int or not 0 <= program < w.programs):
                raise CompatibilityError("Invalid saved program")
            if w.programs:
                w.dispatch(2, value=program)
            for i, value in enumerate(values):
                w.set_parameter(i, value)
        return self.parameter_info() if include_parameters else None

    def process(self, request, audio):
        w = self.runtime
        frames = request["frames"]
        if type(frames) is not int or not 1 <= frames <= w.block_size:
            raise CompatibilityError("Invalid audio frame count")
        if len(audio) != frames * w.inputs * 4:
            raise CompatibilityError("Audio payload size does not match channel/frame count")
        values = struct.unpack(f"<{frames * w.inputs}f", audio)
        if any(not math.isfinite(v) for v in values):
            raise CompatibilityError("Non-finite input audio")
        for index, value in request.get("parameters", []):
            w.set_parameter(index, value)
        channels = [list(values[i * frames:(i + 1) * frames]) for i in range(w.inputs)]
        process = w.diagnose_audio if w.trace_imports else w.process
        output = process(channels, frames, request.get("midi", []))
        return struct.pack(f"<{frames * w.outputs}f", *(v for channel in output for v in channel))

    def close(self):
        self.runtime.close()
