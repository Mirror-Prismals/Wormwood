"""Owned immutable NSArray string collections for opt-in editor diagnostics."""
import ctypes
from contextlib import ExitStack
from format_shims import Arguments
from libc_shims import release
from macho import CompatibilityError


class Arrays:
    def __init__(self, registry):
        self.registry = registry
        self.records = {}

    def create(self, cls):
        from objc_shims import _active_pools
        bridge, w = self.registry.bridge, self.registry.worker
        pools = [p for p in _active_pools if p[0] == bridge.owner_thread]
        if not pools or pools[-1][1] is not bridge:
            raise CompatibilityError('NSArray factory requires an active bridged autorelease pool')
        arguments = Arguments(w, 2)
        objects = []
        for _ in range(1025):
            pointer = arguments.next()
            if not pointer:
                break
            objects.append(pointer)
        else:
            raise CompatibilityError('NSArray exceeds 1024 entries or lacks nil terminator')
        if len(objects) > 1024:
            raise CompatibilityError('NSArray exceeds 1024 entries')
        pins = []
        native = None
        try:
            with ExitStack() as stack:
                values = [stack.enter_context(bridge.string_argument(p)) for p in objects]
                storage = (ctypes.c_void_p * len(values))(*values)
                native = bridge.message(bridge.native.objc_getClass(b'NSArray'), 'alloc')
                if not native:
                    raise CompatibilityError('NSArray allocation failed')
                native = bridge.message_args(native, 'initWithObjects:count:',
                                             [ctypes.c_void_p, ctypes.c_uint64], storage, len(values))
                if not native:
                    raise CompatibilityError('NSArray initialization failed')
            for p in objects:
                if p in bridge.strings:
                    bridge.strings[p]['references'] += 1
                    pins.append(p)
            pointer = self.registry.allocate(16)
            w.write_u64(pointer, cls.pointer)
            self.registry.instances[pointer] = [cls.pointer, 1]
            self.records[pointer] = {'native': native, 'objects': objects, 'pins': pins, 'pending': 1}
            bridge.autoreleases.setdefault(pools[-1][2], []).append((self, pointer))
            return pointer
        except Exception:
            if native:
                bridge.message(native, 'release')
            for p in pins:
                bridge.release_string(p)
            raise

    def release_autoreleased(self, pointer, from_pool=True):
        record = self.records[pointer]
        record['pending'] -= 1
        self.message(pointer, 'release')

    def message(self, pointer, selector):
        record = self.records[pointer]
        registry = self.registry
        if selector == 'count':
            return len(record['objects'])
        if selector == 'objectAtIndex:':
            index = registry.worker.arg(2)
            if index >= len(record['objects']):
                raise CompatibilityError('NSArray index outside bounds')
            return record['objects'][index]
        if selector == 'retain':
            registry.instances[pointer][1] += 1
            return pointer
        if selector == 'release':
            if registry.instances[pointer][1] <= record['pending']:
                raise CompatibilityError('NSArray released with pending autorelease')
            registry.instances[pointer][1] -= 1
            if not registry.instances[pointer][1]:
                registry.bridge.message(record['native'], 'release')
                for p in record['pins']:
                    registry.bridge.release_string(p)
                del self.records[pointer]
                del registry.instances[pointer]
                registry.allocations.remove(pointer)
                release(registry.worker, pointer)
            return 0
        raise CompatibilityError(f'Unsupported NSArray message {selector}')

    def close(self):
        for pointer in list(self.records):
            self.records[pointer]['pending'] = 0
            self.registry.instances[pointer][1] = 1
            self.message(pointer, 'release')
