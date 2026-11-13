"""Worker-local Objective-C class metadata and dispatch to guest Intel methods.

System classes supply read-only name/superclass/size metadata. Plugin classes,
ivars, selectors and methods stay in guest memory. No Intel IMP is registered
with native Objective-C. AppKit object creation/native callbacks remain gated.
"""
import ctypes
import struct
from dataclasses import dataclass, field
from macho import CompatibilityError
from libc_shims import release, string_bytes

SYMBOLS = ("_objc_allocateClassPair", "_objc_registerClassPair", "_objc_disposeClassPair",
           "_objc_lookUpClass", "_objc_getClass", "_class_addMethod", "_class_addIvar",
           "_class_getInstanceVariable", "_class_getSuperclass", "_class_getInstanceSize",
           "_object_getIvar", "_object_setIvar", "_object_getClass", "_ivar_getOffset",
           "_sel_registerName", "_sel_getUid", "_sel_getName")
BUILTINS = tuple("_OBJC_CLASS_$_" + name for name in (
    "NSObject", "NSResponder", "NSView", "NSMenu", "NSMenuItem", "NSTextField",
    "NSWindow", "NSColor", "NSCursor", "NSFont", "NSImage", "NSGraphicsContext",
    "NSPopUpButtonCell", "NSTrackingArea", "NSOpenPanel", "NSSavePanel",
    "NSArray", "NSMutableArray", "NSEvent"))


@dataclass
class TailCall:
    address: int
    receiver: int
    selector: int


@dataclass
class Class:
    name: str
    pointer: int
    parent: int
    size: int
    meta: int = 0
    registered: bool = False
    builtin: bool = False
    is_meta: bool = False
    methods: dict = field(default_factory=dict)
    ivars: dict = field(default_factory=dict)


class Registry:
    def __init__(self, bridge):
        self.bridge, self.worker = bridge, bridge.worker
        self.classes, self.names, self.selectors = {}, {}, {}
        self.ivars, self.instances, self.allocations = {}, {}, set()
        self.native_ready = False
        self.views = None
        self.arrays = None
        self.tracking = None
        self.cursors = None
        self.graphics = None
        self.events = None
        self.windows = None
        if self.worker.editor_views:
            from view_shims import Views
            self.views = Views(self)
            from array_shims import Arrays
            self.arrays = Arrays(self)
            from tracking_shims import TrackingAreas
            self.tracking = TrackingAreas(self)
            from cursor_shims import Cursors
            self.cursors = Cursors(self)
            from graphics_shims import Graphics
            self.graphics = Graphics(self)
            from event_shims import Events
            self.events = Events(self)
            from window_shims import Windows
            self.windows = Windows(self)

    def allocate(self, size):
        pointer = self.worker.allocate(size)
        self.worker.cpu.mem_write(pointer, bytes(size))
        self.allocations.add(pointer)
        return pointer

    def text(self, pointer):
        return string_bytes(self.worker, pointer, limit=1024).decode("utf-8")

    def intern(self, name):
        if name not in self.selectors:
            data = name.encode("utf-8") + b"\0"
            pointer = self.allocate(len(data))
            self.worker.cpu.mem_write(pointer, data)
            self.selectors[name] = pointer
        return self.selectors[name]

    def native_setup(self):
        if self.native_ready:
            return
        self.bridge.load_native()
        # Loading a system framework for class metadata does not open an app,
        # create a window, or execute any Intel plugin code natively.
        self.appkit = ctypes.CDLL("/System/Library/Frameworks/AppKit.framework/AppKit")
        native = self.bridge.native
        for name, result in (("class_getSuperclass", ctypes.c_void_p),
                             ("class_getName", ctypes.c_char_p),
                             ("class_getInstanceSize", ctypes.c_size_t)):
            function = getattr(native, name)
            function.argtypes = [ctypes.c_void_p]
            function.restype = result
        self.native_ready = True

    def builtin(self, name):
        if name in self.names:
            pointer = self.names[name]
            return pointer if self.classes[pointer].registered else 0
        self.native_setup()
        native = self.bridge.native
        cls = native.objc_getClass(name.encode("utf-8"))
        if not cls:
            return 0
        superclass = native.class_getSuperclass(cls)
        parent = self.builtin(native.class_getName(superclass).decode()) if superclass else 0
        symbol = "_OBJC_CLASS_$_" + name
        pointer = self.worker.data_exports.get(symbol) or self.allocate(16)
        meta = self.allocate(16)
        size = native.class_getInstanceSize(cls)
        if not 8 <= size <= 1024 * 1024:
            raise CompatibilityError("Unsupported system Objective-C instance size")
        self.classes[pointer] = Class(name, pointer, parent, size, meta, True, True)
        parent_meta = self.classes[parent].meta if parent else pointer
        root_meta = self.classes[parent_meta].meta if parent else meta
        self.classes[meta] = Class(name + " (meta)", meta, parent_meta, 16, root_meta, True, True, True)
        self.names[name] = pointer
        self.worker.write_u64(pointer, meta)
        self.worker.write_u64(pointer + 8, parent)
        return pointer

    def require(self, pointer):
        if pointer not in self.classes:
            symbol = next((name for name, address in self.worker.data_exports.items()
                           if address == pointer and name.startswith("_OBJC_CLASS_$_")), None)
            if symbol:
                self.builtin(symbol.split("$_", 1)[1])
        if pointer not in self.classes:
            raise CompatibilityError("Unknown guest Objective-C class")
        return self.classes[pointer]

    def inherited(self, pointer, table, name):
        seen = set()
        while pointer and pointer not in seen:
            seen.add(pointer)
            cls = self.require(pointer)
            if name in getattr(cls, table):
                return getattr(cls, table)[name]
            pointer = cls.parent
        return None

    def invoke(self, name):
        w = self.worker
        a, b, c, d, e = (w.arg(i) for i in range(5))
        if name in ("_sel_registerName", "_sel_getUid"):
            return self.intern(self.text(a))
        if name == "_sel_getName":
            return self.intern(self.text(a))
        if name in ("_objc_lookUpClass", "_objc_getClass"):
            return self.builtin(self.text(a))
        if name == "_objc_allocateClassPair":
            parent = self.require(a)
            label = self.text(b)
            if not label or label in self.names or not parent.registered:
                return 0
            if c or len(self.classes) >= 512:
                raise CompatibilityError("Objective-C extra class storage/class count not supported")
            pointer, meta = self.allocate(16), self.allocate(16)
            self.classes[pointer] = Class(label, pointer, a, parent.size, meta)
            self.classes[meta] = Class(label + " (meta)", meta, parent.meta, 16,
                                       self.classes[parent.meta].meta, is_meta=True)
            self.names[label] = pointer
            w.write_u64(pointer, meta)
            w.write_u64(pointer + 8, a)
            return pointer
        if name == "_object_getClass":
            if a in self.classes:
                return self.classes[a].meta
            if a in self.instances:
                return self.instances[a][0]
            raise CompatibilityError("object_getClass requires a known guest object")
        if name == "_ivar_getOffset":
            if a not in self.ivars:
                raise CompatibilityError("Unknown guest instance variable")
            return self.ivars[a][0]
        if name in ("_object_getIvar", "_object_setIvar"):
            if a not in self.instances or b not in self.ivars:
                raise CompatibilityError("Instance-variable access requires a guest object/ivar")
            offset, size, owner = self.ivars[b]
            current = self.instances[a][0]
            while current and current != owner:
                current = self.require(current).parent
            if not current:
                raise CompatibilityError("Instance variable belongs to an unrelated class")
            if size != 8 or offset + 8 > self.require(self.instances[a][0]).size:
                raise CompatibilityError("Unsupported object instance-variable layout")
            if name == "_object_getIvar":
                return w.read_u64(a + offset)
            w.write_u64(a + offset, c)
            return 0
        cls = self.require(a)
        if name == "_class_getSuperclass":
            return cls.parent
        if name == "_class_getInstanceSize":
            return (cls.size + 7) & -8
        if name == "_class_getInstanceVariable":
            return self.inherited(a, "ivars", self.text(b)) or 0
        if cls.builtin:
            raise CompatibilityError("Mutating system Objective-C classes is not supported")
        if name == "_class_addMethod":
            selector, encoding = self.text(b), self.text(d)
            if selector in cls.methods:
                return 0
            if not any(s.protection & 4 and s.address <= c < s.address + s.size
                       for image in w.images for s in image.segments):
                raise CompatibilityError("Objective-C implementation must be executable guest code")
            cls.methods[selector] = (c, encoding)
            self.intern(selector)
            return 1
        if name == "_class_addIvar":
            label, encoding = self.text(b), self.text(e)
            if cls.registered or cls.is_meta or label in cls.ivars:
                return 0
            if not c or c > 1024 * 1024 or d > 16:
                raise CompatibilityError("Unsupported Objective-C ivar size/alignment")
            alignment = 1 << d
            offset = (cls.size + alignment - 1) & -alignment
            if offset + c > 1024 * 1024:
                raise CompatibilityError("Guest Objective-C instance exceeds limit")
            ivar, storage = self.allocate(32), self.allocate(8)
            w.cpu.mem_write(storage, struct.pack("<i", offset))
            w.cpu.mem_write(ivar, struct.pack("<QQQII", storage, self.intern(label), self.intern(encoding), d, c))
            cls.ivars[label] = ivar
            self.ivars[ivar] = (offset, c, a)
            cls.size = offset + c
            return 1
        if name == "_objc_registerClassPair":
            if cls.is_meta:
                raise CompatibilityError("Register the class, not its metaclass")
            cls.registered = self.classes[cls.meta].registered = True
            return 0
        if name == "_objc_disposeClassPair":
            if cls.is_meta:
                raise CompatibilityError("Dispose the class, not its metaclass")
            if any(record[0] == a for record in self.instances.values()) or any(
                    candidate.parent == a for candidate in self.classes.values()):
                raise CompatibilityError("Cannot dispose a class with instances or subclasses")
            del self.names[cls.name]
            del self.classes[cls.meta]
            del self.classes[a]
            return 0
        raise CompatibilityError(f"Unsupported class registry operation {name}")

    def dispatch(self, receiver, selector, superclass=None):
        w = self.worker
        is_class = receiver in self.classes
        if is_class:
            cls = self.classes[receiver]
            lookup = superclass or cls.meta
        elif receiver in self.instances:
            cls = self.require(self.instances[receiver][0])
            lookup = superclass or cls.pointer
        else:
            raise CompatibilityError("Unknown registry Objective-C receiver")
        if not cls.registered:
            raise CompatibilityError("Message sent to an unregistered guest class")
        implementation = self.inherited(lookup, "methods", selector)
        if implementation:
            return TailCall(implementation[0], receiver, self.intern(selector))
        perform = {"performSelector:": 0, "performSelector:withObject:": 1,
                   "performSelector:withObject:withObject:": 2}
        if selector in perform:
            target = self.text(w.arg(2))
            count = perform[selector]
            if target.startswith("performSelector:") or target.count(":") != count:
                raise CompatibilityError("Unsupported recursive or mismatched performSelector call")
            arguments = [w.arg(3 + i) for i in range(count)]
            self.bridge.last_message["performed_selector"] = target
            for register, value in zip((w.x86.UC_X86_REG_RDX, w.x86.UC_X86_REG_RCX), arguments):
                w.cpu.reg_write(register, value)
            # Guest implementations remain TailCalls inside Unicorn. Native
            # objc_msgSend never receives an Intel method implementation.
            return self.dispatch(receiver, target)
        if selector in ("class", "self"):
            return cls.pointer if selector == "class" else receiver
        if self.events is not None and receiver in self.events.records:
            return self.events.message(receiver, selector)
        if self.windows is not None and receiver in self.windows.records:
            return self.windows.message(receiver, selector)
        if self.graphics is not None and cls.name == 'NSGraphicsContext':
            return self.graphics.message(receiver, selector, is_class)
        if self.arrays is not None:
            if is_class and cls.name == "NSArray" and selector == "arrayWithObjects:":
                return self.arrays.create(cls)
            if not is_class and receiver in self.arrays.records:
                return self.arrays.message(receiver, selector)
        if self.cursors is not None:
            if is_class and cls.name == "NSCursor":
                return self.cursors.factory(cls, selector)
            if not is_class and receiver in self.cursors.records:
                return self.cursors.message(receiver, selector)
        if self.tracking is not None:
            if is_class and cls.name == "NSTrackingArea" and selector == "alloc":
                return self.tracking.allocate(cls)
            if not is_class and receiver in self.tracking.records:
                return self.tracking.message(receiver, selector)
        if is_class and selector == "alloc":
            base = cls
            while not base.builtin:
                base = self.require(base.parent)
            if base.name == "NSView" and self.views is not None:
                if self.inherited(cls.meta, "methods", "initialize"):
                    raise CompatibilityError("Guest +initialize dispatch is not implemented")
                return self.views.allocate(cls)
            if base.name != "NSObject" or cls.builtin:
                raise CompatibilityError(f"Native {base.name} instances/callbacks are not implemented")
            if self.inherited(cls.meta, "methods", "initialize"):
                raise CompatibilityError("Guest +initialize dispatch is not implemented")
            pointer = self.allocate((cls.size + 7) & -8)
            w.write_u64(pointer, cls.pointer)
            self.instances[pointer] = [cls.pointer, 1]
            return pointer
        if not is_class and self.views is not None and receiver in self.views.records:
            return self.views.message(receiver, selector)
        if not is_class and selector == "init":
            return receiver
        if not is_class and selector in ("retain", "release"):
            self.instances[receiver][1] += 1 if selector == "retain" else -1
            if not self.instances[receiver][1]:
                if self.inherited(cls.pointer, "methods", "dealloc"):
                    self.instances[receiver][1] = 1
                    raise CompatibilityError("Guest dealloc callbacks are not implemented")
                del self.instances[receiver]
                self.allocations.remove(receiver)
                release(w, receiver)
            return receiver if selector == "retain" else 0
        raise CompatibilityError(f"Unsupported registry message {cls.name} {selector}")

    def close(self):
        if self.windows is not None:
            self.windows.close()
        if self.events is not None:
            self.events.close()
        if self.graphics is not None:
            self.graphics.close()
        if self.views is not None:
            self.views.close()
        if self.arrays is not None:
            self.arrays.close()
        if self.tracking is not None:
            self.tracking.close()
        if self.cursors is not None:
            self.cursors.close()
        for pointer in self.allocations:
            release(self.worker, pointer)
        self.allocations.clear()
        self.instances.clear()
