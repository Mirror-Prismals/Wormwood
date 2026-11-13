"""Explicit CoreFoundation string/URL bridge; native pointers never enter guest memory."""
import ctypes
from contextlib import contextmanager
from macho import CompatibilityError
from libc_shims import release, string_bytes

CONSTANTS = ("_kCFBooleanTrue", "_kCFBooleanFalse", "_kCGImageSourceShouldCache",
             "_kCFTypeDictionaryKeyCallBacks", "_kCFTypeDictionaryValueCallBacks", "_kCGImagePropertyHasAlpha",
             "_kCGColorSpaceGenericRGB")

SIGNATURES = {
    "CFDictionaryGetValue": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
    "CFDictionaryCreate": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_void_p, ctypes.c_void_p]),
    "CFURLGetFileSystemRepresentation": (ctypes.c_uint8, [ctypes.c_void_p, ctypes.c_uint8, ctypes.c_void_p, ctypes.c_int64]),
    "CFRelease": (None, [ctypes.c_void_p]),
    "CFRetain": (ctypes.c_void_p, [ctypes.c_void_p]),
    "CFStringCreateWithCString": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint32]),
    "CFStringCreateWithCharacters": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64]),
    "CFStringGetCString": (ctypes.c_uint8, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_uint32]),
    "CFStringGetSystemEncoding": (ctypes.c_uint32, []),
    "CFURLCreateFromFSRef": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
    "CFURLCopyFileSystemPath": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_int64]),
    "CFURLCreateCopyDeletingLastPathComponent": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
    "CFURLCreateFromFileSystemRepresentation": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_uint8]),
    "CFURLCreateWithFileSystemPath": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_uint8]),
    "CFURLCreateWithBytes": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int64, ctypes.c_uint32, ctypes.c_void_p]),
}
SYMBOLS = tuple("_" + name for name in SIGNATURES) + ("_CFStringGetCStringPtr",)


class CoreFoundation:
    def __init__(self, worker):
        self.worker, self.native = worker, None
        self.objects = {}
        self.constants = set()
        self.last_dictionary = None
        self.dictionary_pins = {}

    def load_native(self):
        if self.native:
            return
        self.native = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
        for name, (result, args) in SIGNATURES.items():
            function = getattr(self.native, name)
            function.restype, function.argtypes = result, args

    def install_constants(self, exports):
        self.load_native()
        image_io = ctypes.CDLL("/System/Library/Frameworks/ImageIO.framework/ImageIO")
        graphics = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
        for symbol, kind in (("_kCFBooleanTrue", "boolean"), ("_kCFBooleanFalse", "boolean"),
                             ("_kCGImageSourceShouldCache", "string"), ("_kCGImagePropertyHasAlpha", "string"),
                             ("_kCGColorSpaceGenericRGB", "string")):
            library = graphics if symbol == '_kCGColorSpaceGenericRGB' else image_io if symbol.startswith("_kCG") else self.native
            native = ctypes.c_void_p.in_dll(library, symbol[1:]).value
            pointer = self.wrap(self.native.CFRetain(native), kind)
            self.constants.add(pointer)
            self.worker.write_u64(exports[symbol], pointer)

    def wrap(self, native, kind):
        if not native:
            return 0
        try:
            pointer = self.worker.allocate(16)
        except Exception:
            self.native.CFRelease(native)
            raise
        self.worker.cpu.mem_write(pointer, bytes(16))
        self.objects[pointer] = [native, kind, 1]
        return pointer

    def get(self, pointer, kind=None):
        if pointer not in self.objects or kind and self.objects[pointer][1] != kind:
            actual = self.objects[pointer][1] if pointer in self.objects else "unregistered"
            raise CompatibilityError(f"Unknown or incompatible guest CoreFoundation object 0x{pointer:x}: expected {kind or 'object'}, got {actual}")
        return self.objects[pointer][0]

    @contextmanager
    def string(self, pointer):
        if pointer in self.objects:
            yield self.get(pointer, "string")
        else:
            with self.worker.objc.string_argument(pointer) as native:
                yield native

    def invoke(self, symbol):
        w = self.worker
        name = symbol[1:]
        a, b, c, d, e = (w.arg(i) for i in range(5))
        self.load_native()
        if name in ("CFRelease", "CFRetain"):
            self.get(a)
            if a in self.constants:
                return a if name == "CFRetain" else 0
            self.objects[a][2] += 1 if name == "CFRetain" else -1
            if not self.objects[a][2]:
                self.native.CFRelease(self.objects.pop(a)[0])
                for native in self.dictionary_pins.pop(a, []):
                    self.native.CFRelease(native)
                release(w, a)
            return a if name == "CFRetain" else 0
        if name == "CFDictionaryGetValue":
            dictionary = self.get(a, "dictionary")
            with self.string(b) as key:
                value = self.native.CFDictionaryGetValue(dictionary, key)
            if not value:
                return 0
            # Preserve pointer identity for exported CFBoolean singletons and
            # other already bridged objects. Never leak a borrowed native pointer.
            for pointer, (native, _, _) in self.objects.items():
                if native == value:
                    return pointer
            raise CompatibilityError("CFDictionary value type has no borrowed-object bridge yet")
        if name == "CFStringGetSystemEncoding":
            return self.native.CFStringGetSystemEncoding()
        if name in ("CFStringGetCString", "CFStringGetCStringPtr"):
            with self.string(a) as native:
                if name == "CFStringGetCStringPtr":
                    # CF permits NULL when no suitable direct C representation
                    # is available. Callers must use the copying conversion.
                    return 0
                if c > 4 * 1024 * 1024:
                    raise CompatibilityError("CFString output buffer exceeds limit")
                if c == 0:
                    return 0
                output = ctypes.create_string_buffer(c)
                success = self.native.CFStringGetCString(native, output, c, d & 0xffffffff)
                if success:
                    w.cpu.mem_write(b, output.raw)
                return success
        if name == "CFURLGetFileSystemRepresentation":
            if not 1 <= d <= 65536:
                raise CompatibilityError("CFURL path buffer exceeds limit")
            output = ctypes.create_string_buffer(d)
            result = self.native.CFURLGetFileSystemRepresentation(self.get(a, "url"), bool(b), output, d)
            if result:
                w.cpu.mem_write(c, output.value + b"\0")
            return result
        if name == "CFURLCopyFileSystemPath":
            if b not in (0, 1, 2):
                raise CompatibilityError("Unsupported CFURL path style")
            return self.wrap(self.native.CFURLCopyFileSystemPath(self.get(a, "url"), b), "string")
        if a not in (0, self.system_allocator):
            raise CompatibilityError("Custom CoreFoundation allocators are not supported")
        allocator = ctypes.c_void_p.in_dll(self.native, "kCFAllocatorSystemDefault").value if a else None
        if name == "CFDictionaryCreate":
            if d > 1024:
                raise CompatibilityError("CFDictionary entry count exceeds limit")
            key_callbacks, value_callbacks = e, w.arg(5)
            self.last_dictionary = {"count": d, "key_callbacks": key_callbacks, "value_callbacks": value_callbacks,
                                    "keys": [w.read_u64(b + i * 8) for i in range(d)],
                                    "values": [w.read_u64(c + i * 8) for i in range(d)]}
            callbacks = []
            for guest, symbol in ((key_callbacks, "_kCFTypeDictionaryKeyCallBacks"),
                                  (value_callbacks, "_kCFTypeDictionaryValueCallBacks")):
                if guest == 0:
                    callbacks.append(None)
                elif guest == w.data_exports[symbol]:
                    callbacks.append(ctypes.addressof(ctypes.c_byte.in_dll(self.native, symbol[1:])))
                else:
                    raise CompatibilityError("Custom CFDictionary callbacks are not supported")
            pins = []
            def entry(pointer):
                if pointer in self.objects:
                    return self.get(pointer)
                # Constant CFStrings and bridged NSStrings also satisfy CF's
                # object contract. Keep translations alive even with null
                # dictionary callbacks, which do not retain the entries.
                with self.string(pointer) as native:
                    retained = self.native.CFRetain(native)
                    pins.append(retained)
                    return retained
            try:
                keys = (ctypes.c_void_p * d)(*(entry(w.read_u64(b + i * 8)) for i in range(d)))
                values = (ctypes.c_void_p * d)(*(entry(w.read_u64(c + i * 8)) for i in range(d)))
                handle = self.wrap(self.native.CFDictionaryCreate(allocator, keys, values, d, *callbacks), "dictionary")
                if handle:
                    self.dictionary_pins[handle] = pins
                    pins = []
                return handle
            finally:
                for native in pins:
                    self.native.CFRelease(native)
        if name == "CFStringCreateWithCString":
            return self.wrap(self.native.CFStringCreateWithCString(allocator, string_bytes(w, b), c & 0xffffffff), "string")
        if name == "CFStringCreateWithCharacters":
            if c > 65536:
                raise CompatibilityError("CFString input length exceeds limit")
            data = w.read(b, c * 2)
            return self.wrap(self.native.CFStringCreateWithCharacters(allocator, data, c), "string")
        if name == "CFURLCreateFromFSRef":
            return self.wrap(self.native.CFURLCreateFromFSRef(allocator, w.filesystem.reference(b)), "url")
        if name == "CFURLCreateCopyDeletingLastPathComponent":
            return self.wrap(self.native.CFURLCreateCopyDeletingLastPathComponent(allocator, self.get(b, "url")), "url")
        if name == "CFURLCreateWithFileSystemPath":
            if c not in (0, 1, 2):
                raise CompatibilityError("Unsupported CFURL path style")
            with self.string(b) as native:
                return self.wrap(self.native.CFURLCreateWithFileSystemPath(allocator, native, c, bool(d)), "url")
        if name in ("CFURLCreateFromFileSystemRepresentation", "CFURLCreateWithBytes"):
            if c > 65536:
                raise CompatibilityError("CFURL input length exceeds limit")
            data = w.read(b, c)
            if name == "CFURLCreateWithBytes":
                native = self.native.CFURLCreateWithBytes(allocator, data, c, d & 0xffffffff, self.get(e, "url") if e else None)
            else:
                native = self.native.CFURLCreateFromFileSystemRepresentation(allocator, data, c, bool(d))
            return self.wrap(native, "url")
        raise CompatibilityError(f"Unsupported CoreFoundation function {name}")

    def close(self):
        for pointer, (native, _, _) in list(self.objects.items()):
            self.native.CFRelease(native)
            release(self.worker, pointer)
            del self.objects[pointer]
        for pins in self.dictionary_pins.values():
            for native in pins:
                self.native.CFRelease(native)
        self.dictionary_pins.clear()
        self.constants.clear()
