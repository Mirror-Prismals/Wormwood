"""i386 implementation of the existing serialized audio-worker contract."""
import hashlib
import struct

from macho import CompatibilityError
from macho32 import MachO32
from runtime32 import Runtime32
from session import Session, MAX_STATE


class Session32(Session):
    pointer_size = 4

    def __init__(self, request, *, editor_support=False):
        if request.get('libraries'):
            raise CompatibilityError('Shared i386 guest libraries are not implemented')
        self.image = MachO32.from_path(request['plugin'])
        self.identity = hashlib.sha256(self.image.data).hexdigest()
        self.runtime = Runtime32(self.image, resource_root=request.get('resource_root'),
                                 startup_seconds=request.get('startup_seconds', 15), editor_support=editor_support)
        self.state_storage = None
        # Unknown imports remain fail-on-use guards; never substitute a result.
        self.runtime.trace_imports = False
        try:
            self.runtime.initialize()
            self.runtime.open_plugin()
            if not editor_support:
                self.runtime.start_audio(request.get('sample_rate', 48000), request.get('block_size', 256))
        except Exception:
            self.runtime.close(suppress=True)
            raise

    def parameter_info(self, indices=None):
        return self.runtime.parameter_info(indices)

    def describe(self):
        w = self.runtime
        identity, version = struct.unpack('<ii', w.read(w.effect + 72, 8))
        return dict(architecture='i386', binary_sha256=self.identity, unique_id=identity,
                    plugin_version=version, inputs=w.inputs, outputs=w.outputs,
                    is_instrument=bool(w.flags & 256), sample_rate=w.sample_rate,
                    block_size=w.block_size, latency_frames=struct.unpack('<i', w.read(w.effect + 48, 4))[0],
                    parameters=self.parameter_info(), programs=w.programs,
                    state_kind='chunk' if w.flags & 32 else 'parameters',
                    experimental=False, unresolved_imports=sorted(w.missing),
                    native_editor_supported=False, rosetta_used=False)

    def state_buffer(self, data):
        if len(data) > MAX_STATE:
            raise CompatibilityError('i386 state exceeds size limit')
        if self.state_storage is None:
            self.state_storage = self.runtime.allocate(MAX_STATE)
        self.runtime.cpu.mem_write(self.state_storage, data)
        return self.state_storage

    def state_pointer(self, address):
        return self.runtime.read_u32(address)
