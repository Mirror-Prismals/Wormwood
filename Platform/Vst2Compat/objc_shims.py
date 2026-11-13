"""Narrow native-ARM Foundation bridge with guest object handles.

Only explicit pool and string messages are supported so far. Intel objects,
selectors, and function pointers are never passed to the native runtime.
Unknown classes/messages fail explicitly; there is no generic objc_msgSend FFI.
"""
import ctypes
from contextlib import contextmanager
import struct
import threading
import objc_registry
from macho import CompatibilityError
from libc_shims import release, string_bytes

SYMBOLS = ("_objc_msgSend", "_objc_msgSend_fixup", "_objc_msgSendSuper",
           "_objc_msgSend_stret", "_objc_msgSend_stret_fixup") + objc_registry.SYMBOLS
CLASS_SYMBOL = "_OBJC_CLASS_$_NSAutoreleasePool"
STRING_CLASSES = ("_OBJC_CLASS_$_NSString", "_OBJC_CLASS_$_NSMutableString")
CONSTANT_STRING_CLASS = "___CFConstantStringClassReference"
APPKIT_STRINGS = ("_NSFilenamesPboardType", "_NSStringPboardType")
_active_pools = []  # native pools nest per thread, including across guest workers


class ObjectiveC:
    def __init__(self, worker, class_address):
        self.worker, self.class_address = worker, class_address
        self.owner_thread = threading.get_ident()
        self.objects = {}
        self.strings, self.autoreleases = {}, {}
        self.native = None
        self.last_message = None
        self.registry = objc_registry.Registry(self)
        worker.write_u64(class_address, class_address)  # opaque guest class handle

    def load_native(self):
        if self.native is not None:
            return
        # Runtime has already required native ARM64 macOS. Only system ARM code
        # is loaded here, never the Intel plugin or its guest C++ dependency.
        self.foundation = ctypes.CDLL("/System/Library/Frameworks/Foundation.framework/Foundation")
        self.native = ctypes.CDLL("/usr/lib/libobjc.A.dylib")
        self.native.objc_getClass.argtypes = [ctypes.c_char_p]
        self.native.objc_getClass.restype = ctypes.c_void_p
        self.native.sel_registerName.argtypes = [ctypes.c_char_p]
        self.native.sel_registerName.restype = ctypes.c_void_p
        self.send = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)(
            ("objc_msgSend", self.native))
        self.native_class = self.native.objc_getClass(b"NSAutoreleasePool")
        if not self.native_class:
            raise CompatibilityError("Native Foundation has no NSAutoreleasePool class")

    def message(self, receiver, selector):
        return self.send(receiver, self.native.sel_registerName(selector.encode("ascii")))

    def install_appkit_strings(self):
        self.registry.native_setup()
        w = self.worker
        for symbol in APPKIT_STRINGS:
            native = ctypes.c_void_p.in_dll(self.registry.appkit, symbol[1:]).value
            raw = self.message(native, "UTF8String")
            if not raw:
                raise CompatibilityError("AppKit string constant has no UTF-8 representation")
            size = self.message_args(native, "lengthOfBytesUsingEncoding:", (ctypes.c_uint64,), 4) or 0
            if size > 4096:
                raise CompatibilityError("AppKit string constant exceeds limit")
            data = ctypes.string_at(raw, size)
            storage = self.registry.allocate(size + 1)
            w.cpu.mem_write(storage, data + b"\0")
            literal = self.registry.allocate(32)
            w.cpu.mem_write(literal, struct.pack("<QI4xQQ", w.data_exports[CONSTANT_STRING_CLASS],
                                                0x7c8, storage, size))
            w.write_u64(w.data_exports[symbol], literal)

    def message_args(self, receiver, selector, types, *arguments):
        send = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, *types)(
            ("objc_msgSend", self.native))
        return send(receiver, self.native.sel_registerName(selector.encode("ascii")), *arguments)

    def same_thread(self):
        if threading.get_ident() != self.owner_thread:
            raise CompatibilityError("Objective-C pool used from a different worker thread")

    def invoke(self, name):
        self.same_thread()
        w = self.worker
        if name in objc_registry.SYMBOLS:
            return self.registry.invoke(name)
        if name in ("_objc_msgSend_stret", "_objc_msgSend_stret_fixup"):
            destination, receiver, selector = w.arg(0), w.arg(1), w.arg(2)
            if name.endswith('_fixup'):
                selector = w.read_u64(selector + 8)
            selector = string_bytes(w, selector, limit=1024).decode('utf-8')
            self.last_message = {'receiver_address': receiver, 'selector': selector, 'structure_return': True}
            if self.registry.views is None:
                raise CompatibilityError('Structure-return view queries require the editor bridge')
            return self.registry.views.return_rect(destination, receiver, selector)
        if name == "_objc_msgSendSuper":
            receiver, superclass = struct.unpack("<QQ", w.read(w.arg(0), 16))
            selector = string_bytes(w, w.arg(1), limit=1024).decode("utf-8")
            return self.registry.dispatch(receiver, selector, superclass=superclass)
        receiver, selector = w.arg(0), w.arg(1)
        if name == "_objc_msgSend_fixup":
            selector = w.read_u64(selector + 8)
        selector = string_bytes(w, selector, limit=1024).decode("utf-8", errors="replace")
        self.last_message = {"receiver_address": receiver, "selector": selector}
        for symbol, address in w.missing_addresses.items():
            if receiver == address:
                self.last_message["receiver_import"] = symbol
                break
        string_class = next((symbol for symbol in STRING_CLASSES if w.data_exports[symbol] == receiver), None)
        if string_class or receiver in self.strings:
            return self.string_message(receiver, selector, string_class)
        if any(receiver == w.data_exports[symbol] for symbol in objc_registry.BUILTINS):
            self.registry.require(receiver)
        if receiver != self.class_address and (receiver in self.registry.classes or receiver in self.registry.instances):
            return self.registry.dispatch(receiver, selector)
        if receiver == self.class_address:
            self.last_message["class"] = "NSAutoreleasePool"
            if selector not in ("alloc", "allocWithZone:", "new"):
                raise CompatibilityError(f"Unsupported +[NSAutoreleasePool {selector}]")
            if selector == "allocWithZone:" and w.arg(2):
                raise CompatibilityError("Objective-C custom allocation zones are not supported")
            pointer = w.allocate(16)
            w.cpu.mem_write(pointer, bytes(16))
            w.write_u64(pointer, self.class_address)
            # Native NSAutoreleasePool allocation can use a cached placeholder.
            # Keep alloc as an owned guest handle; create the native pool only
            # at init so abandoned, uninitialized handles cannot double-drain it.
            self.objects[pointer] = [None, False]
            return self.initialize(pointer) if selector == "new" else pointer
        if receiver not in self.objects:
            label = self.last_message.get("receiver_import", f"0x{receiver:x}")
            raise CompatibilityError(f"Unsupported Objective-C receiver {label} for {selector}")
        self.last_message["class"] = "NSAutoreleasePool"
        if selector == "init":
            return self.initialize(receiver)
        if selector in ("release", "drain"):
            self.destroy(receiver, selector)
            return 0
        if selector == "self":
            return receiver
        if selector == "class":
            return self.class_address
        raise CompatibilityError(f"Unsupported -[NSAutoreleasePool {selector}]")

    def initialize(self, pointer):
        native, active = self.objects[pointer]
        if active:
            raise CompatibilityError("Objective-C pool initialized more than once")
        self.load_native()
        native = self.message(self.native_class, "alloc")
        initialized = self.message(native, "init")
        if not initialized:
            del self.objects[pointer]
            release(self.worker, pointer)
            return 0
        self.objects[pointer] = [initialized, True]
        _active_pools.append((self.owner_thread, self, pointer))
        return pointer

    def destroy(self, pointer, selector="release"):
        self.same_thread()
        native, active = self.objects[pointer]
        thread_pools = [pool for pool in _active_pools if pool[0] == self.owner_thread]
        if active and (not thread_pools or thread_pools[-1][1:] != (self, pointer)):
            raise CompatibilityError("Objective-C pools must drain in reverse initialization order")
        if active:
            for owner, object_pointer in self.autoreleases.pop(pointer, []):
                owner.release_autoreleased(object_pointer, from_pool=True)
            self.message(native, selector)
            _active_pools.remove((self.owner_thread, self, pointer))
        del self.objects[pointer]
        release(self.worker, pointer)

    def close(self):
        # Reverse initialization order (which need not equal allocation order).
        for thread, owner, pointer in reversed(list(_active_pools)):
            if owner is self:
                self.destroy(pointer)
        for pointer in list(self.objects):
            self.destroy(pointer)
        self.registry.close()
        for pointer in list(self.strings):
            self.strings[pointer]["references"] = 1
            self.release_string(pointer)

    def release_autoreleased(self, pointer, from_pool=True):
        self.release_string(pointer, from_pool=from_pool)

    def release_string(self, pointer, from_pool=False):
        record = self.strings.get(pointer)
        if record is None:
            raise CompatibilityError("Autoreleased guest string was already destroyed")
        if from_pool:
            record["pending"] -= 1
        elif record["references"] - 1 < record["pending"]:
            raise CompatibilityError("Guest string release would invalidate a pending autorelease")
        record["references"] -= 1
        if record["references"]:
            return
        if record["native"]:
            self.message(record["native"], "release")
        for buffer in record["buffers"]:
            release(self.worker, buffer)
        del self.strings[pointer]
        release(self.worker, pointer)

    @contextmanager
    def string_argument(self, pointer):
        record = self.strings.get(pointer)
        if record and record["native"]:
            yield record["native"]
            return
        w = self.worker
        isa, flags, data, length = struct.unpack("<QI4xQQ", w.read(pointer, 32))
        if isa != w.data_exports[CONSTANT_STRING_CLASS] or flags not in (0x7c8, 0x7d0) or length > 65536:
            raise CompatibilityError("Unsupported guest constant-string layout")
        # Clang's legacy 64-bit CFString literal: isa, flags, data, length.
        # Decode guest bytes first; no guest pointer reaches native Foundation.
        encoding = "ascii" if flags == 0x7c8 else "utf-16-le"
        try:
            text = w.read(data, length * (1 if flags == 0x7c8 else 2)).decode(encoding).encode("utf-8")
        except UnicodeError as exc:
            raise CompatibilityError("Invalid guest constant-string encoding") from exc
        self.load_native()
        native = self.message(self.native.objc_getClass(b"NSString"), "alloc")
        native = self.message_args(native, "initWithBytes:length:encoding:",
                                   (ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64), text, len(text), 4)
        if not native:
            raise CompatibilityError("Could not create native constant-string argument")
        try:
            yield native
        finally:
            self.message(native, "release")

    def string_message(self, receiver, selector, class_symbol):
        w = self.worker
        autorelease_result = False
        if class_symbol:
            self.last_message["class"] = class_symbol.split("$_", 1)[1]
            factories = {"string": "init", "stringWithString:": "initWithString:",
                         "stringWithUTF8String:": "initWithUTF8String:"}
            if selector not in ("alloc", "allocWithZone:", "new", *factories):
                raise CompatibilityError(f"Unsupported string class message {selector}")
            if selector == "allocWithZone:" and w.arg(2):
                raise CompatibilityError("Objective-C custom allocation zones are not supported")
            receiver = w.allocate(16)
            w.cpu.mem_write(receiver, bytes(16))
            w.write_u64(receiver, w.data_exports[class_symbol])
            self.strings[receiver] = {"native": None, "class": class_symbol,
                                      "references": 1, "pending": 0, "buffers": []}
            if selector in factories:
                autorelease_result = True
                selector = factories[selector]
            elif selector != "new":
                return receiver
            else:
                selector = "init"
        record = self.strings[receiver]
        self.last_message["class"] = record["class"].split("$_", 1)[1]
        if selector in ("init", "initWithCapacity:", "initWithUTF8String:", "initWithString:"):
            if record["native"]:
                raise CompatibilityError("Guest string initialized more than once")
            if selector == "initWithCapacity:" and record["class"] != STRING_CLASSES[1]:
                raise CompatibilityError("Capacity initializer requires NSMutableString")
            capacity = w.arg(2)
            if selector == "initWithCapacity:" and capacity > 65536:
                raise CompatibilityError("Guest string capacity exceeds limit")
            text = string_bytes(w, w.arg(2)) if selector == "initWithUTF8String:" else None
            self.load_native()
            native_class = self.native.objc_getClass(self.last_message["class"].encode("ascii"))
            if selector == "initWithString:":
                with self.string_argument(w.arg(2)) as argument:
                    native = self.message(native_class, "alloc")
                    native = self.message_args(native, selector, (ctypes.c_void_p,), argument)
            elif selector == "initWithCapacity:":
                native = self.message(native_class, "alloc")
                native = self.message_args(native, selector, (ctypes.c_uint64,), capacity)
            elif text is not None:
                native = self.message(native_class, "alloc")
                native = self.message_args(native, selector, (ctypes.c_char_p,), text)
            else:
                native = self.message(native_class, "alloc")
                native = self.message(native, "init")
            if not native:
                self.release_string(receiver)
                return 0
            record["native"] = native
            if autorelease_result:
                self.string_message(receiver, "autorelease", None)
            return receiver
        if selector == "release":
            self.release_string(receiver)
            return 0
        if selector == "retain":
            record["references"] += 1
            return receiver
        if selector == "autorelease":
            pools = [pool for pool in _active_pools if pool[0] == self.owner_thread]
            if not pools:
                raise CompatibilityError("Guest string autorelease requires an active bridged pool")
            _, owner, pool = pools[-1]
            if owner is not self:
                raise CompatibilityError("Autorelease across guest workers is not supported")
            if record["pending"] >= record["references"]:
                raise CompatibilityError("Guest string autoreleased more times than retained")
            owner.autoreleases.setdefault(pool, []).append((self, receiver))
            record["pending"] += 1
            return receiver
        if selector == "self":
            return receiver
        if selector == "class":
            return w.data_exports[record["class"]]
        if not record["native"]:
            raise CompatibilityError("Guest string used before initialization")
        if selector == "length":
            return self.message(record["native"], selector) or 0
        if selector == "UTF8String":
            if record["buffers"]:
                return record["buffers"][0]
            size = self.message_args(record["native"], "lengthOfBytesUsingEncoding:",
                                     (ctypes.c_uint64,), 4) or 0  # NSUTF8StringEncoding
            if size > 262144:
                raise CompatibilityError("Guest string UTF-8 representation exceeds limit")
            native = self.message(record["native"], selector)
            if not native:
                return 0
            data = ctypes.string_at(native, size) + b"\0"
            pointer = w.allocate(len(data))
            w.cpu.mem_write(pointer, data)
            record["buffers"].append(pointer)
            return pointer
        if selector in ("appendString:", "setString:"):
            if record["class"] != STRING_CLASSES[1]:
                raise CompatibilityError("Mutation requires NSMutableString")
            with self.string_argument(w.arg(2)) as argument:
                size = self.message(argument, "length") or 0
                if selector == "appendString:":
                    size += self.message(record["native"], "length") or 0
                if size > 65536:
                    raise CompatibilityError("Guest string length exceeds limit")
                self.message_args(record["native"], selector, (ctypes.c_void_p,), argument)
            for buffer in record["buffers"]:
                release(w, buffer)
            record["buffers"].clear()
            return 0
        raise CompatibilityError(f"Unsupported string instance message {selector}")
