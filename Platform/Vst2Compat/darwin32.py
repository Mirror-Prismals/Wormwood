"""Worker-local Darwin bookkeeping with 32-bit guest pointers."""
import struct
import ctypes
from dataclasses import dataclass

from macho import CompatibilityError

SYMBOLS = ('__keymgr_get_and_lock_processwide_ptr', '__keymgr_get_and_lock_processwide_ptr_2',
           '__keymgr_set_and_unlock_processwide_ptr', '__dyld_image_count',
           '__dyld_get_image_header', '__dyld_get_image_name', '__dyld_get_image_vmaddr_slide',
           '___cxa_guard_acquire', '___cxa_guard_release', '___cxa_guard_abort',
           '_dlopen', '_dlsym', '_dlclose', '___cxa_atexit', '_atexit', '___cxa_finalize',
           '_MPCreateCriticalRegion', '_MPDeleteCriticalRegion', '_MPEnterCriticalRegion', '_MPExitCriticalRegion', '_TickCount', '_GetDblTime')


@dataclass
class CallBatch:
    callbacks: list
    completion: object = None


class Darwin32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.keys, self.locks, self.guards = {}, set(), set()
        self.image_name = 0
        self.host_header = 0
        self.host_name = 0
        self.handles = {}
        self.next_handle = 0x75300000
        self.dynamic_lookups = []
        self.exit_handlers = []
        self.regions = {}
        self.next_region = 0x75400000
        self.tick_count = None
        self.double_click_time = None

    def string(self, address):
        result = bytearray()
        for offset in range(4096):
            byte = self.runtime.read(address + offset, 1)
            if byte == b'\0':
                return result.decode('utf-8')
            result.extend(byte)
        raise CompatibilityError('Unterminated i386 dynamic-loader string')

    def invoke(self, name, arg):
        w = self.runtime
        if name == '_GetDblTime':
            if self.double_click_time is None:
                library = ctypes.CDLL('/System/Library/Frameworks/Carbon.framework/Carbon')
                self.double_click_time = library.GetDblTime
                self.double_click_time.argtypes, self.double_click_time.restype = [], ctypes.c_uint32
            return self.double_click_time()
        if name == '_TickCount':
            if self.tick_count is None:
                library = ctypes.CDLL('/System/Library/Frameworks/CoreServices.framework/CoreServices')
                self.tick_count = library.TickCount
                self.tick_count.argtypes, self.tick_count.restype = [], ctypes.c_uint32
            return self.tick_count()
        if name == '_MPCreateCriticalRegion':
            if len(self.regions) >= 1024 or self.next_region >= 0x75500000:
                raise CompatibilityError('i386 critical-region limit exceeded')
            token = self.next_region
            self.next_region += 16
            w.cpu.mem_write(arg(0), struct.pack('<I', token))
            self.regions[token] = 0
            return 0
        if name in ('_MPDeleteCriticalRegion', '_MPEnterCriticalRegion', '_MPExitCriticalRegion'):
            token = arg(0)
            if token not in self.regions:
                return -50  # paramErr
            depth = self.regions[token]
            # The offline worker serializes all guest calls; MP task creation
            # is unsupported. Recursive entries belong to the same guest task.
            if name == '_MPEnterCriticalRegion':
                if depth >= 1024:
                    raise CompatibilityError('i386 critical-region recursion limit exceeded')
                self.regions[token] = depth + 1
            elif name == '_MPExitCriticalRegion':
                if not depth:
                    raise CompatibilityError('i386 critical region exited without acquisition')
                self.regions[token] = depth - 1
            else:
                if depth:
                    raise CompatibilityError('i386 critical region deleted while held')
                del self.regions[token]
            return 0
        if name in ('___cxa_atexit', '_atexit'):
            function = arg(0)
            if not function or len(self.exit_handlers) >= 4096:
                return -1
            self.exit_handlers.append((function, (arg(1),) if name == '___cxa_atexit' else (),
                                       arg(2) if name == '___cxa_atexit' else 0))
            return 0
        if name == '___cxa_finalize':
            dso = arg(0)
            selected = [(function, arguments) for function, arguments, owner in reversed(self.exit_handlers)
                        if not dso or owner == dso]
            self.exit_handlers = [entry for entry in self.exit_handlers if dso and entry[2] != dso]
            return CallBatch(selected)
        if name == '_dlopen':
            library = self.string(arg(0)) if arg(0) else '<guest>'
            if library not in ('/usr/lib/libSystem.B.dylib', '<guest>'):
                raise CompatibilityError('Dynamic i386 library loading is not implemented: ' + library)
            if len(self.handles) >= 128 or self.next_handle >= 0x75400000:
                raise CompatibilityError('i386 dynamic-loader handle limit exceeded')
            handle = self.next_handle
            self.next_handle += 16
            self.handles[handle] = library
            return handle
        if name in ('_dlsym', '_dlclose'):
            handle = arg(0)
            if handle not in self.handles:
                raise CompatibilityError('Unknown i386 dynamic-loader handle')
            if name == '_dlclose':
                del self.handles[handle]
                return 0
            symbol = '_' + self.string(arg(1))
            if len(self.dynamic_lookups) < 128:
                self.dynamic_lookups.append(symbol)
            if symbol in w.shims:
                return w.shims[symbol]
            if symbol in w.data_exports:
                return w.data_exports[symbol]
            if self.handles[handle] == '<guest>' and symbol in w.image.exports:
                return w.image.exports[symbol]
            # Unknown mandatory symbols must be implemented deliberately. Do
            # not hand an ARM function address to Intel code or silently skip it.
            raise CompatibilityError('Unimplemented i386 dynamic symbol: ' + symbol)
        if name.startswith('__keymgr_'):
            key = arg(0)
            if name == '__keymgr_set_and_unlock_processwide_ptr':
                if key not in self.locks:
                    return 1  # EPERM
                self.keys[key] = arg(1)
                self.locks.remove(key)
                return 0
            if key in self.locks:
                if name.endswith('_2'):
                    return 11  # Darwin EDEADLK
                raise CompatibilityError('Recursive i386 key-manager lock')
            self.locks.add(key)
            value = self.keys.get(key, 0)
            if name.endswith('_2'):
                w.cpu.mem_write(arg(1), struct.pack('<I', value))
                return 0
            return value
        if name == '__dyld_image_count':
            return 2 if w.editor_support else 1
        if name.startswith('__dyld_get_image_'):
            index = arg(0)
            if w.editor_support and index == 0:
                if not self.host_header:
                    self.host_header = w.allocate(28)
                    w.cpu.mem_write(self.host_header, struct.pack('<7I', 0xfeedface, 7, 3, 2, 0, 0, 0))
                    self.host_name = w.allocate(32)
                    w.cpu.mem_write(self.host_name, b'/Cardinal/compatibility-host\0')
                return self.host_header if name == '__dyld_get_image_header' else self.host_name if name == '__dyld_get_image_name' else 0
            if index != (1 if w.editor_support else 0):
                return 0
            if name == '__dyld_get_image_vmaddr_slide':
                return 0
            if name == '__dyld_get_image_header':
                return next((s.address for s in w.image.segments if s.offset == 0 and s.file_size >= 28), 0)
            if not self.image_name:
                path = getattr(w.image, 'path', None)
                if path is None:
                    raise CompatibilityError('i386 guest image has no filesystem identity')
                data = path.encode('utf-8') + b'\0'
                self.image_name = w.allocate(len(data))
                w.cpu.mem_write(self.image_name, data)
            return self.image_name
        guard = arg(0)
        if name == '___cxa_guard_acquire':
            if w.read(guard, 1) != b'\0':
                return 0
            if guard in self.guards:
                raise CompatibilityError('Recursive i386 C++ static initialization')
            self.guards.add(guard)
            return 1
        if guard not in self.guards:
            raise CompatibilityError('i386 C++ guard released without acquisition')
        self.guards.remove(guard)
        if name == '___cxa_guard_release':
            w.cpu.mem_write(guard, b'\x01')
        return 0
