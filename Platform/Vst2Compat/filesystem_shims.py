"""Read-only Carbon folder/path queries with worker-owned opaque FSRef tokens."""
from collections import deque
import ctypes
import struct
from macho import CompatibilityError
from libc_shims import release, string_bytes

SYMBOLS = ("_FSFindFolder", "_FSRefMakePath", "_FSPathMakeRef", "_FSOpenIterator", "_FSCloseIterator", "_FSGetCatalogInfoBulk")


class Filesystem:
    def __init__(self, worker):
        self.worker = worker
        self.references = {}
        self.iterators = {}
        self.native = None
        self.queries = deque(maxlen=64)

    def load_native(self):
        if self.native:
            return
        self.native = ctypes.CDLL("/System/Library/Frameworks/CoreServices.framework/CoreServices")
        for name, args in (
                ("FSOpenIterator", [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p]),
                ("FSCloseIterator", [ctypes.c_void_p]),
                ("FSGetCatalogInfoBulk", [ctypes.c_void_p, ctypes.c_uint64, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]),
                ("FSFindFolder", [ctypes.c_int16, ctypes.c_uint32, ctypes.c_uint8, ctypes.c_void_p]),
                ("FSRefMakePath", [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32]),
                ("FSPathMakeRef", [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_void_p])):
            function = getattr(self.native, name)
            function.argtypes = args
            function.restype = ctypes.c_int16 if name in ("FSOpenIterator", "FSCloseIterator", "FSGetCatalogInfoBulk") else ctypes.c_int32

    def save_reference(self, destination, native):
        if len(self.references) >= 4096:
            raise CompatibilityError("Too many guest filesystem references")
        token = b"SLMFSREF" + struct.pack("<Q", len(self.references) + 1) + bytes(64)
        self.worker.cpu.mem_write(destination, token)
        self.references[token] = bytes(native)

    def reference(self, pointer):
        token = self.worker.read(pointer, 80)
        if token not in self.references:
            raise CompatibilityError("Unknown guest FSRef")
        return ctypes.create_string_buffer(self.references[token], 80)

    def invoke(self, name):
        w = self.worker
        a, b, c, d = (w.arg(i) for i in range(4))
        self.load_native()
        if name == "_FSOpenIterator":
            if w.read(a, 80) not in self.references:
                return -1401  # errFSBadFSRef; never pass guest bytes to native code
            if b != 0:
                raise CompatibilityError("Only flat read-only directory iteration is supported")
            if len(self.iterators) >= 64:
                raise CompatibilityError("Too many directory iterators")
            native = ctypes.c_void_p()
            result = self.native.FSOpenIterator(self.reference(a), b, ctypes.byref(native))
            if result == 0:
                pointer = None
                try:
                    pointer = w.allocate(16)
                    w.write_u64(c, pointer)
                except Exception:
                    self.native.FSCloseIterator(native)
                    if pointer is not None:
                        release(w, pointer)
                    raise
                self.iterators[pointer] = native
            return result
        if name in ("_FSCloseIterator", "_FSGetCatalogInfoBulk"):
            if a not in self.iterators:
                raise CompatibilityError("Unknown guest directory iterator")
            if name == "_FSCloseIterator":
                result = self.native.FSCloseIterator(self.iterators[a])
                if result == 0:
                    del self.iterators[a]
                    release(w, a)
                return result
            if not 1 <= b <= 256:
                raise CompatibilityError("Directory batch must contain 1..256 entries")
            mask, catalog, refs, specs, names = (w.arg(i) for i in range(4, 9))
            if mask & ~0x000bffff:
                raise CompatibilityError("Unsupported directory catalog fields (including security objects)")
            # SDK-checked on both x86_64 and ARM64: FSCatalogInfo=148,
            # HFSUniStr255=512, ItemCount=8; fileSec pointer at byte 68.
            infos = ctypes.create_string_buffer(148 * b) if catalog else None
            native_refs = ctypes.create_string_buffer(80 * b) if refs else None
            native_names = ctypes.create_string_buffer(512 * b) if names else None
            actual, changed = ctypes.c_uint64(), ctypes.c_uint8()
            result = self.native.FSGetCatalogInfoBulk(self.iterators[a], b, ctypes.byref(actual),
                        ctypes.byref(changed), mask, infos, native_refs, None, native_names)
            if actual.value > b:
                raise CompatibilityError("Directory API returned an oversized batch")
            w.write_u64(c, actual.value)
            if d:
                w.cpu.mem_write(d, bytes([changed.value]))
            for index in range(actual.value):
                if catalog:
                    data = bytearray(infos.raw[index * 148:(index + 1) * 148])
                    # Never export a native object pointer, including unsolicited fields.
                    if any(data[68:76]):
                        raise CompatibilityError("Unexpected native file security object")
                    w.cpu.mem_write(catalog + index * 148, bytes(data))
                if refs:
                    self.save_reference(refs + index * 80, native_refs.raw[index * 80:(index + 1) * 80])
                if names:
                    w.cpu.mem_write(names + index * 512, native_names.raw[index * 512:(index + 1) * 512])
            return result  # errFSNoMoreItems can accompany a final nonempty batch.
        if name == "_FSFindFolder":
            if c & 255:
                raise CompatibilityError("Folder creation is outside the read-only filesystem bridge")
            native = ctypes.create_string_buffer(80)
            result = self.native.FSFindFolder(a & 0xffff, b & 0xffffffff, 0, native)
            self.queries.append({"api": "FSFindFolder", "domain": ctypes.c_int16(a).value,
                                 "folder_type": (b & 0xffffffff).to_bytes(4, "big").decode("ascii", errors="replace"),
                                 "status": result})
            if result == 0:
                self.save_reference(d, native)
            return result
        if name == "_FSPathMakeRef":
            path = string_bytes(w, a, limit=4096)
            native, directory = ctypes.create_string_buffer(80), ctypes.c_uint8()
            result = self.native.FSPathMakeRef(w.resource_paths.resolve(path), native, ctypes.byref(directory))
            self.queries.append({"api": "FSPathMakeRef", "path": path.decode("utf-8", errors="replace"), "status": result})
            if result == 0:
                self.save_reference(b, native)
                if c:
                    w.cpu.mem_write(c, bytes([directory.value]))
            return result
        if name == "_FSRefMakePath":
            if not 1 <= c <= 65536:
                raise CompatibilityError("FSRef path buffer exceeds supported bounds")
            output = ctypes.create_string_buffer(c)
            result = self.native.FSRefMakePath(self.reference(a), output, c)
            if result == 0:
                w.cpu.mem_write(b, output.value + b"\0")
            return result
        raise CompatibilityError(f"Unsupported filesystem operation {name}")

    def close(self):
        for pointer, native in list(self.iterators.items()):
            self.native.FSCloseIterator(native)
            release(self.worker, pointer)
            del self.iterators[pointer]
        self.references.clear()
