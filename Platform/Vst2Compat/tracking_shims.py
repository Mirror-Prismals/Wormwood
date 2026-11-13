"""Offscreen NSTrackingArea setup; guest mouse-event callbacks are not wired yet."""
import ctypes
import struct
from libc_shims import release
from macho import CompatibilityError
from view_shims import Point, Size, Rect


class TrackingAreas:
    def __init__(self, registry):
        self.registry = registry
        self.records = {}

    def allocate(self, cls):
        bridge = self.registry.bridge
        native = bridge.message(bridge.native.objc_getClass(b'NSTrackingArea'), 'alloc')
        if not native:
            raise CompatibilityError('NSTrackingArea allocation failed')
        try:
            pointer = self.registry.allocate(16)
        except Exception:
            bridge.message(native, 'release')
            raise
        self.registry.worker.write_u64(pointer, cls.pointer)
        self.registry.instances[pointer] = [cls.pointer, 1]
        self.records[pointer] = {'native': native, 'initialized': False, 'pending': 0, 'view': 0}
        return pointer

    def release_autoreleased(self, pointer, from_pool=True):
        self.records[pointer]['pending'] -= 1
        self.message(pointer, 'release')

    def message(self, pointer, selector):
        registry, w = self.registry, self.registry.worker
        bridge = registry.bridge
        record = self.records[pointer]
        if selector == 'initWithRect:options:owner:userInfo:':
            import math
            frame = struct.unpack('<4d', w.read(w.cpu.reg_read(w.x86.UC_X86_REG_RSP) + 8, 32))
            options, owner, info = w.arg(2), w.arg(3), w.arg(4)
            if record['initialized'] or not all(math.isfinite(v) and abs(v) <= 1e6 for v in frame) or not all(0 <= v <= 16384 for v in frame[2:]):
                raise CompatibilityError('Invalid tracking-area initialization')
            active = options & 0xf0
            if (owner not in registry.views.records or info or options & ~0x7f7
                    or not options & 7 or active not in (0x10, 0x20, 0x40, 0x80)
                    or options & 4 and active == 0x80):
                raise CompatibilityError('Tracking area requires bridged view owner, bounded options and nil userInfo')
            native = bridge.message_args(record['native'], selector,
                [Rect, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_void_p],
                Rect(Point(*frame[:2]), Size(*frame[2:])), options, registry.views.records[owner]['native'], None)
            record['native'] = native
            if not native:
                raise CompatibilityError('NSTrackingArea initialization failed')
            # NSTrackingArea does not retain its owner. Pin only the plain native
            # backing until this area dies, including deferred autoreleases.
            owner_native = registry.views.records[owner]['native']
            bridge.message(owner_native, 'retain')
            record.update(initialized=True, owner=owner, owner_native=owner_native, frame=frame, options=options)
            return pointer
        if selector == 'retain':
            registry.instances[pointer][1] += 1
            return pointer
        if selector == 'autorelease':
            from objc_shims import _active_pools
            pools = [p for p in _active_pools if p[0] == bridge.owner_thread]
            if not pools or pools[-1][1] is not bridge or record['pending'] >= registry.instances[pointer][1]:
                raise CompatibilityError('Invalid tracking-area autorelease')
            bridge.autoreleases.setdefault(pools[-1][2], []).append((self, pointer))
            record['pending'] += 1
            return pointer
        if selector == 'release':
            if registry.instances[pointer][1] <= record['pending']:
                raise CompatibilityError('Tracking area released with pending autorelease')
            registry.instances[pointer][1] -= 1
            if not registry.instances[pointer][1]:
                bridge.message(record['native'], 'release')
                if record.get('owner_native'):
                    bridge.message(record['owner_native'], 'release')
                del self.records[pointer]
                del registry.instances[pointer]
                registry.allocations.remove(pointer)
                release(w, pointer)
            return 0
        raise CompatibilityError(f'Unsupported NSTrackingArea message {selector}')

    def close(self):
        for pointer in list(self.records):
            self.records[pointer]['pending'] = 0
            self.registry.instances[pointer][1] = 1
            self.message(pointer, 'release')
