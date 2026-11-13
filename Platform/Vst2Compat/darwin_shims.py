"""Legacy Darwin bookkeeping, C++ teardown, and a typed native tick clock.

Single-threaded by contract. dlopen/dlsym below refer solely to explicitly
implemented virtual runtime exports; they never load a host or guest dylib.
"""
import ctypes
from dataclasses import dataclass
from macho import CompatibilityError
from libc_shims import release, string_bytes

SYMBOLS = ("__keymgr_get_and_lock_processwide_ptr", "__keymgr_get_and_lock_processwide_ptr_2",
           "__keymgr_set_and_unlock_processwide_ptr", "_dlopen", "_dlsym", "_dlclose",
           "___cxa_atexit", "___cxa_finalize", "_atexit",
           "___cxa_guard_acquire", "___cxa_guard_release", "___cxa_guard_abort",
           "_pthread_mutex_init", "_pthread_mutex_destroy", "_pthread_mutex_lock",
           "_pthread_mutex_trylock", "_pthread_mutex_unlock", "_pthread_once", "_TickCount", "__dyld_image_count", "__dyld_get_image_header",
           "__dyld_get_image_name", "__dyld_get_image_vmaddr_slide", "_clock")


@dataclass
class GuestCalls:
    calls: object  # iterable of (guest function address, integer arguments)
    result: int = 0


class Darwin:
    def __init__(self, worker):
        self.worker = worker
        self.tick_count = None
        self.cpu_clock = None
        self.image_names = {}
        self.keys, self.locks, self.handles = {}, set(), {}
        self.exit_handlers = []
        self.guard_locks = set()
        self.mutex_locks = set()

    def finalize(self, dso):
        # Pop before invoking. Handlers registered by a destructor are also
        # considered on the next iteration, without running a handler twice.
        while True:
            index = next((i for i in range(len(self.exit_handlers) - 1, -1, -1)
                          if not dso or self.exit_handlers[i][2] == dso), None)
            if index is None:
                return
            function, argument, _, has_argument = self.exit_handlers.pop(index)
            yield function, (argument,) if has_argument else ()

    def invoke(self, name):
        if name == '_clock':
            if self.cpu_clock is None:
                self.cpu_clock = ctypes.CDLL('/usr/lib/libSystem.B.dylib').clock
                # Darwin arm64 and x86_64 both use unsigned long clock_t.
                # Calling libc preserves its CPU-time units and failure value.
                self.cpu_clock.argtypes, self.cpu_clock.restype = [], ctypes.c_ulong
            return self.cpu_clock()
        if name == "_TickCount":
            if self.tick_count is None:
                library = ctypes.CDLL("/System/Library/Frameworks/CoreServices.framework/CoreServices")
                self.tick_count = library.TickCount
                self.tick_count.argtypes, self.tick_count.restype = [], ctypes.c_uint32
            return self.tick_count()
        w = self.worker
        a, b, c = w.arg(0), w.arg(1), w.arg(2)
        if name == "__dyld_image_count":
            return len(w.images)
        if name.startswith("__dyld_get_image_"):
            index = a & 0xffffffff
            if index >= len(w.images):
                return 0
            image = w.images[index]
            if name == "__dyld_get_image_vmaddr_slide":
                return 0  # Loader maps all images at their preferred addresses.
            if name == "__dyld_get_image_header":
                return next((segment.address for segment in image.segments
                             if segment.offset == 0 and segment.file_size >= 32), 0)
            if index not in self.image_names:
                path = getattr(image, "path", None)
                if path is None:
                    raise CompatibilityError("Guest image has no filesystem identity")
                data = path.encode("utf-8") + b"\0"
                pointer = w.allocate(len(data))
                w.cpu.mem_write(pointer, data)
                self.image_names[index] = pointer
            return self.image_names[index]
        if name == "_pthread_once":
            if w.read_u64(a) != 0x30B1BCBA or not b:
                raise CompatibilityError("Unsupported Darwin pthread_once control")
            state = w.read_u64(a + 8)
            if state == 2:
                return 0
            if state != 0:
                raise CompatibilityError("Recursive or invalid pthread_once initialization")
            def initialize_once():
                w.write_u64(a + 8, 1)
                try:
                    yield b, ()
                    w.write_u64(a + 8, 2)
                finally:
                    if w.read_u64(a + 8) == 1:
                        w.write_u64(a + 8, 0)
            return GuestCalls(initialize_once())
        if name.startswith("_pthread_mutex_"):
            # Darwin x86_64 pthread_mutex_t: 8-byte signature + 56 opaque bytes.
            # Only default nonrecursive mutexes, in this single-threaded worker.
            blob = w.read(a, 64)
            if name == "_pthread_mutex_init":
                if b:
                    raise CompatibilityError("pthread mutex attributes are not implemented")
                if a in self.mutex_locks:
                    return 16  # EBUSY
                w.cpu.mem_write(a, (0x32AAABA7).to_bytes(8, "little") + bytes(56))
                return 0
            if int.from_bytes(blob[:8], "little") != 0x32AAABA7:
                raise CompatibilityError("Unsupported or uninitialized Darwin mutex")
            if name == "_pthread_mutex_destroy":
                if a in self.mutex_locks:
                    return 16
                w.cpu.mem_write(a, bytes(64))
                return 0
            if name == "_pthread_mutex_unlock":
                if a not in self.mutex_locks:
                    return 1  # EPERM
                self.mutex_locks.remove(a)
                return 0
            if a in self.mutex_locks:
                if name == "_pthread_mutex_trylock":
                    return 16
                raise CompatibilityError("Guest would deadlock on a nonrecursive mutex")
            self.mutex_locks.add(a)
            return 0
        if name.startswith("__keymgr_"):
            key = a & 0xffffffff
            if name == "__keymgr_set_and_unlock_processwide_ptr":
                if key not in self.locks:
                    return 1  # EPERM
                self.keys[key] = b
                self.locks.remove(key)
                return 0
            if key in self.locks:
                if name.endswith("_2"):
                    return 11  # Darwin EDEADLK
                raise CompatibilityError("Recursive legacy key-manager lock")
            self.locks.add(key)
            value = self.keys.get(key, 0)
            if name.endswith("_2"):
                w.write_u64(b, value)
                return 0
            return value
        if name == "_dlopen":
            library = string_bytes(w, a).decode("utf-8") if a else "<guest>"
            if library not in ("/usr/lib/libSystem.B.dylib", "<guest>"):
                raise CompatibilityError(f"Dynamic library loading is not implemented: {library}")
            handle = w.allocate(16)
            self.handles[handle] = library
            return handle
        if name == "_dlsym":
            if a not in self.handles:
                raise CompatibilityError("Unsupported dlsym handle")
            symbol = "_" + string_bytes(w, b).decode("utf-8")
            # Names requested from libSystem must not resolve back into the
            # plugin's older CRT wrappers of the same name.
            if self.handles[a] == "<guest>" and symbol in w.exports:
                return w.exports[symbol]
            if symbol in w.data_exports:
                return w.data_exports[symbol]
            if symbol not in w.SHIMS:
                raise CompatibilityError(f"Unimplemented dynamic runtime export: {symbol}")
            return w.shim_address(symbol)
        if name == "_dlclose":
            if a not in self.handles:
                raise CompatibilityError("Unsupported dlclose handle")
            del self.handles[a]
            from libc_shims import release
            release(w, a)
            return 0
        if name in ("___cxa_atexit", "_atexit"):
            if not a or len(self.exit_handlers) >= 4096:
                return -1
            self.exit_handlers.append((a, b if name == "___cxa_atexit" else 0,
                                       c if name == "___cxa_atexit" else 0,
                                       name == "___cxa_atexit"))
            return 0
        if name == "___cxa_finalize":
            return GuestCalls(self.finalize(a))
        if name == "___cxa_guard_acquire":
            if w.read(a, 1) != b"\0":
                return 0
            if a in self.guard_locks:
                raise CompatibilityError("Recursive C++ static initialization")
            self.guard_locks.add(a)
            return 1
        if name in ("___cxa_guard_release", "___cxa_guard_abort"):
            if a not in self.guard_locks:
                raise CompatibilityError("C++ guard released without acquisition")
            self.guard_locks.remove(a)
            if name == "___cxa_guard_release":
                w.cpu.mem_write(a, b"\1")
            return 0
        raise CompatibilityError(f"Unimplemented Darwin shim: {name}")

    def close(self):
        for pointer in self.image_names.values():
            release(self.worker, pointer)
        self.image_names.clear()
        for pointer in self.handles:
            release(self.worker, pointer)
        self.handles.clear()
