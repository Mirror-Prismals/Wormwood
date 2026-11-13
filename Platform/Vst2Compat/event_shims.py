"""Worker-local diagnostic mouse events; never inject events into macOS apps."""
import math
import struct
from libc_shims import release
from macho import CompatibilityError


class Events:
    def __init__(self, registry):
        self.registry = registry
        self.records = {}

    def create(self, kind, location, timestamp, window=0):
        if (kind not in (1, 2, 6) or not math.isfinite(timestamp) or timestamp < 0
                or not all(math.isfinite(v) and abs(v) <= 1e6 for v in location)):
            raise CompatibilityError('Invalid diagnostic mouse event')
        r = self.registry
        if window and window not in r.windows.records:
            raise CompatibilityError('Unknown diagnostic event window')
        cls = r.builtin('NSEvent')
        pointer = r.allocate(16)
        r.worker.write_u64(pointer, cls)
        r.instances[pointer] = [cls, 1]
        self.records[pointer] = dict(kind=kind, location=tuple(location), timestamp=timestamp, window=window)
        return pointer

    def message(self, pointer, selector):
        r, event = self.registry, self.records[pointer]
        w = r.worker
        if selector == 'locationInWindow':
            for i, value in enumerate(event['location']):
                w.cpu.reg_write(getattr(w.x86, f'UC_X86_REG_XMM{i}'), struct.unpack('<Q', struct.pack('<d', value))[0])
            return 0
        if selector == 'timestamp':
            w.cpu.reg_write(w.x86.UC_X86_REG_XMM0, struct.unpack('<Q', struct.pack('<d', event['timestamp']))[0])
            return 0
        values = {'type': event['kind'], 'modifierFlags': 0, 'buttonNumber': 0,
                  'clickCount': 1, 'windowNumber': r.windows.message(event['window'], 'windowNumber') if event['window'] else 0,
                  'window': event['window']}
        if selector in values:
            return values[selector]
        if selector == 'retain':
            r.instances[pointer][1] += 1
            return pointer
        if selector == 'release':
            r.instances[pointer][1] -= 1
            if not r.instances[pointer][1]:
                del self.records[pointer]
                del r.instances[pointer]
                r.allocations.remove(pointer)
                release(w, pointer)
            return 0
        raise CompatibilityError(f'Unsupported diagnostic NSEvent message {selector}')

    def close(self):
        for pointer in list(self.records):
            self.registry.instances[pointer][1] = 1
            self.message(pointer, 'release')
