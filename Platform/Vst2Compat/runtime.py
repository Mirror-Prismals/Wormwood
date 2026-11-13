"""Experimental x86_64 VST2 worker. Guest code only executes inside Unicorn.

Offline proof of execution, not a realtime host or a security sandbox. There is
no dlopen of plugin code, Intel subprocess, Rosetta launch, or native fallback.
"""
import math
import ctypes
import platform
import struct
import time
from collections import deque
from functools import partial
import libc_shims
import darwin_shims
import stdio_shims
import locale_shims
import objc_shims
import objc_registry
import format_shims
import filesystem_shims
import cf_shims
import math_shims
import image_shims
import graphics_shims

from macho import CompatibilityError, MAX_IMAGE
from resource_paths import ResourcePaths

PAGE = 4096
STACK = 0x10000000000
STACK_SIZE = 1024 * 1024
HEAP = STACK + 0x1000000
HEAP_SIZE = 32 * 1024 * 1024
SCRATCH = HEAP + HEAP_SIZE
SCRATCH_SIZE = 4 * 1024 * 1024
GATES = SCRATCH + SCRATCH_SIZE
RETURN = GATES + PAGE - 1
RUNTIME_DATA = GATES + PAGE
DATA_EXPORTS = {name: RUNTIME_DATA + i * 8 for i, name in enumerate(stdio_shims.DATA_NAMES)}
DATA_EXPORTS[objc_shims.CLASS_SYMBOL] = RUNTIME_DATA + 1024
DATA_EXPORTS.update({name: RUNTIME_DATA + 1040 + index * 16
                     for index, name in enumerate(objc_shims.STRING_CLASSES)})
DATA_EXPORTS[objc_shims.CONSTANT_STRING_CLASS] = RUNTIME_DATA + 1080
DATA_EXPORTS.update({name: RUNTIME_DATA + 1104 + index * 16
                     for index, name in enumerate(objc_registry.BUILTINS)})
DATA_EXPORTS["_kCFAllocatorSystemDefault"] = RUNTIME_DATA + 2048
DATA_EXPORTS["_kCFAllocatorDefault"] = RUNTIME_DATA + 2056
DATA_EXPORTS.update({name: RUNTIME_DATA + 2200 + index * 64
                     for index, name in enumerate(cf_shims.CONSTANTS)})
DATA_EXPORTS.update({name: RUNTIME_DATA + 3000 + index * 8
                     for index, name in enumerate(objc_shims.APPKIT_STRINGS)})
MASK = (1 << 64) - 1
MISSING_BASE = 0x30000000000
MISSING_STRIDE = 65536


class Runtime:
    # Unknown imports fail before execution unless explicit diagnostic tracing
    # is selected, in which case their first access faults on an unmapped guard.
    SHIMS = (("_malloc", "_calloc", "_free", "_memcpy", "_memmove", "_memset", "_strlen",
              "_bzero", "___bzero", "_memchr", "_time", "_srand", "_rand", "_rand_r", "dyld_stub_binder")
             + libc_shims.SYMBOLS + darwin_shims.SYMBOLS + stdio_shims.SYMBOLS + locale_shims.SYMBOLS + objc_shims.SYMBOLS + format_shims.SYMBOLS + filesystem_shims.SYMBOLS + cf_shims.SYMBOLS + math_shims.SYMBOLS + image_shims.SYMBOLS + graphics_shims.SYMBOLS)

    def __init__(self, image, sample_rate=48000, block_size=256, trace_imports=False, libraries=(), startup_seconds=2, resource_root=None, editor_views=False):
        if any(getattr(current, "architecture", "x86_64") != "x86_64" for current in (image, *libraries)):
            raise CompatibilityError("32-bit images require the separate i386 runtime; x86_64 execution refused")
        self.resource_paths = ResourcePaths(resource_root)
        self.editor_views = editor_views
        if platform.machine().lower() not in ("arm64", "aarch64"):
            raise CompatibilityError("Run this proof with native ARM64 Python; Intel/Rosetta Python is rejected")
        if not math.isfinite(sample_rate) or not 8000 <= sample_rate <= 384000 or not 1 <= block_size <= 8192:
            raise CompatibilityError("Unsupported sample rate or block size")
        if not math.isfinite(startup_seconds) or not 0.1 <= startup_seconds <= 30:
            raise CompatibilityError("Startup call budget must be 0.1..30 seconds")
        self.startup_seconds, self.closing = startup_seconds, False
        self.images = [image, *libraries]
        if len(self.images) > 16:
            raise CompatibilityError("Too many explicit guest libraries")
        if any(current.kind != 6 for current in self.images[1:]):
            raise CompatibilityError("Explicit guest libraries must be Mach-O dylibs")
        ranges = []
        self.exports, weak_exports = {}, set()
        for current in self.images:
            if current.unsupported:
                raise CompatibilityError("Unsupported loader features: " + ", ".join(sorted(set(current.unsupported))))
            for segment in current.segments:
                if segment.size:
                    if segment.address % PAGE:
                        raise CompatibilityError("Unaligned Mach-O segment")
                    ranges.append((segment.address, segment.address + ((segment.size + PAGE - 1) & -PAGE)))
            for name, address in current.exports.items():
                weak = name in current.weak_definitions
                if name in self.exports and name not in weak_exports:
                    if not weak:
                        raise CompatibilityError(f"Ambiguous strong guest export: {name}")
                    continue
                if name not in self.exports or not weak:
                    self.exports[name] = address
                    if weak:
                        weak_exports.add(name)
                    else:
                        weak_exports.discard(name)
        ranges.sort()
        if sum(end - start for start, end in ranges) > MAX_IMAGE:
            raise CompatibilityError("Combined guest images exceed memory limit")
        if any(a[1] > b[0] for a, b in zip(ranges, ranges[1:])):
            raise CompatibilityError("Overlapping guest image mappings; image sliding is not implemented")
        if image.entry is None:
            raise CompatibilityError("VST entry symbol not found; export-trie-only images are not supported yet")
        self.bindings = []
        missing = set()
        for current in self.images:
            bindings = list(current.bindings())
            weak_bindings = list(current.weak_bindings())
            for name in current.imports:
                if name not in current.symbols and name not in self.exports and name not in self.SHIMS and name not in DATA_EXPORTS:
                    missing.add(name)
            for is_weak, entries in ((False, bindings), (True, weak_bindings)):
                for address, name, addend in entries:
                    if not is_weak and name in current.symbols and name not in current.weak_definitions:
                        resolved = current.symbols[name]
                    elif name in self.SHIMS and (name not in self.exports or name in weak_exports):
                        resolved = self.shim_address(name)
                    elif name in self.exports:
                        resolved = self.exports[name]
                    elif name in DATA_EXPORTS:
                        resolved = DATA_EXPORTS[name]
                    else:
                        resolved = None
                        missing.add(name)
                    self.bindings.append((address, name, addend, resolved))
        missing = sorted(missing)
        if missing and not trace_imports:
            raise CompatibilityError("Unimplemented Mac imports: " + ", ".join(missing))
        self.missing_imports = missing
        self.trace_imports = trace_imports
        self.phase = "mapping"
        self.completed_initializers = 0
        self.total_initializers = sum(len(current.initializers) for current in self.images)
        self.recent_imports = deque(maxlen=24)
        self.import_counts = {}
        self.objective_c_message = None
        self.missing_import_call = None
        self.missing_addresses = {name: MISSING_BASE + i * MISSING_STRIDE + MISSING_STRIDE // 2
                                  for i, name in enumerate(missing)}
        try:
            import unicorn as uc
            from unicorn import x86_const as x86
        except ImportError as exc:
            raise CompatibilityError("Unicorn is not installed; see Platform/Vst2Compat/README.md") from exc
        self.uc, self.x86 = uc, x86
        self.cpu = uc.Uc(uc.UC_ARCH_X86, uc.UC_MODE_64)
        self.sample_rate, self.block_size = sample_rate, block_size
        self.position = 0
        self.effect = 0
        self.opened = self.active = self.started = False
        self.heap_next, self.scratch_next = HEAP, SCRATCH
        self.allocations, self.free_blocks = {}, []
        self.callback_error = None
        self.host_calls = {}
        self.unknown_host_calls = set()
        self.image = image
        self.darwin = darwin_shims.Darwin(self)
        self.call_depth = 0
        self.pending_calls = None
        self.runtime_finalized = False
        self.random_state, self.native_random = 1, None
        for current in self.images:
            for segment in current.segments:
                if not segment.size:
                    continue
                self.cpu.mem_map(segment.address, (segment.size + PAGE - 1) & -PAGE, segment.protection)
                if segment.file_size:
                    self.cpu.mem_write(segment.address, current.data[segment.offset:segment.offset + segment.file_size])
        self.cpu.mem_map(STACK, STACK_SIZE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.cpu.mem_map(HEAP, HEAP_SIZE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.cpu.mem_map(SCRATCH, SCRATCH_SIZE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.cpu.mem_map(GATES, PAGE, uc.UC_PROT_READ | uc.UC_PROT_EXEC)
        self.cpu.mem_map(RUNTIME_DATA, PAGE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.stdio = stdio_shims.Stdio(self, RUNTIME_DATA)
        self.data_exports = DATA_EXPORTS
        self.objc = objc_shims.ObjectiveC(self, DATA_EXPORTS[objc_shims.CLASS_SYMBOL])
        self.filesystem = filesystem_shims.Filesystem(self)
        self.cf = cf_shims.CoreFoundation(self)
        self.math = math_shims.Math(self)
        self.resource_images = image_shims.Images(self)
        # Resolve the owning module once instead of scanning symbol tuples on
        # every imported call (hundreds of thousands during some plugin starts).
        self.shim_handlers = {}
        for module, handler in (
                (libc_shims, partial(libc_shims.invoke, self)),
                (darwin_shims, self.darwin.invoke),
                (image_shims, self.resource_images.invoke),
                (graphics_shims, self.graphics_call),
                (stdio_shims, self.stdio.invoke),
                (locale_shims, partial(locale_shims.invoke, self)),
                (objc_shims, self.objc.invoke),
                (format_shims, partial(format_shims.invoke, self)),
                (filesystem_shims, self.filesystem.invoke),
                (cf_shims, self.cf.invoke),
                (math_shims, self.math.invoke)):
            for symbol in module.SYMBOLS:
                self.shim_handlers.setdefault(symbol, handler)
        self.cf.system_allocator = RUNTIME_DATA + 2080
        self.cf.install_constants(DATA_EXPORTS)
        self.objc.install_appkit_strings()
        self.write_u64(DATA_EXPORTS["_kCFAllocatorSystemDefault"], self.cf.system_allocator)
        for name in objc_shims.STRING_CLASSES:
            self.write_u64(DATA_EXPORTS[name], DATA_EXPORTS[name])
        self.cpu.mem_write(GATES, b"\xc3" * PAGE)  # emulated RET after a host shim
        self.gates = {GATES: self.host_callback}
        for i, name in enumerate(self.SHIMS, 1):
            self.gates[GATES + i * 16] = lambda name=name: self.shim(name)
        self.cpu.hook_add(uc.UC_HOOK_CODE, self.on_gate, begin=GATES, end=RETURN - 1)
        self.cpu.hook_add(uc.UC_HOOK_INSN, self.on_syscall, None, 1, 0, x86.UC_X86_INS_SYSCALL)
        self.cpu.hook_add(uc.UC_HOOK_INTR, self.on_interrupt)
        self.cpu.hook_add(uc.UC_HOOK_MEM_INVALID, self.on_invalid_memory)
        for address, name, addend, resolved in self.bindings:
            if resolved is None:
                # Diagnostic mode only: unresolved functions AND data point into
                # separate unmapped guard regions. First use stops emulation;
                # nothing returns dummy success or emulates a missing object.
                resolved = self.missing_addresses[name]
            self.write_u64(address, resolved + addend)
        # Images stay at their preferred addresses, so classic pointer rebases
        # have zero slide. Chained fixups and other relocation forms are rejected.
        self.time_info = self.allocate(88)
        try:
            # Explicit libraries are supplied in dependency initialization order.
            # No native loader, recursive library discovery, or image sliding.
            for index, current in [*enumerate(self.images[1:], 1), (0, image)]:
                for i, address in enumerate(current.initializers):
                    prefix = f"library {index} " if index else ""
                    self.phase = f"{prefix}initializer {i + 1}/{len(current.initializers)}"
                    self.call(self.read_u64(address))
                    self.completed_initializers += 1
            self.phase = "VST entry"
            self.effect = self.call(image.entry, GATES)
            if not self.effect:
                raise CompatibilityError("VSTPluginMain returned null")
            self.refresh_effect()
            self.phase = "VST open"
            self.dispatch(0)
            self.opened = True
            self.refresh_effect()
            self.phase = "VST configure"
            self.dispatch(10, opt=float(sample_rate))
            self.dispatch(11, value=block_size)
            self.dispatch(12, value=1)
            self.active = True
            self.refresh_effect()
            self.dispatch(71)
            self.started = True
            self.phase = "ready"
        except Exception as exc:
            # Preserve the failure before cleanup attempts can alter diagnostics.
            if isinstance(exc, CompatibilityError):
                exc.details = self.diagnostics()
            self.close(suppress=True)
            raise

    def diagnostics(self):
        return {"phase": self.phase, "completed_initializers": self.completed_initializers,
                "total_initializers": self.total_initializers, "guest_image_count": len(self.images),
                "recent_imports": list(self.recent_imports), "import_calls": dict(self.import_counts),
                "unimplemented_import_count": len(self.missing_imports),
                "captured_stdout_bytes": len(self.stdio.output[1]),
                "captured_stderr_bytes": len(self.stdio.output[2]),
                "objective_c_message": self.objective_c_message or self.objc.last_message,
                "missing_import_call": self.missing_import_call,
                "folder_queries": list(self.filesystem.queries),
                "file_queries": list(self.stdio.open_queries),
                "image_queries": list(self.resource_images.queries),
                "last_cf_dictionary": self.cf.last_dictionary,
                "editor_cursor": self.objc.registry.cursors.requested_style() if self.objc.registry.cursors else None,
                "editor_views": self.objc.registry.views.diagnostics() if self.objc.registry.views else [],
                "startup_call_budget_seconds": self.startup_seconds,
                "guest_pc": self.cpu.reg_read(self.x86.UC_X86_REG_RIP),
                "guest_objc_classes": [{"name": cls.name, "registered": cls.registered,
                                         "methods": len(cls.methods), "ivars": len(cls.ivars)}
                                        for pointer, cls in self.objc.registry.classes.items()
                                        if not cls.builtin and not cls.is_meta],
                "diagnostic_mode": self.trace_imports, "rosetta_used": False}

    def shim_address(self, name):
        return GATES + (self.SHIMS.index(name) + 1) * 16

    def read(self, address, size):
        if not 0 <= size <= SCRATCH_SIZE:
            raise CompatibilityError("Guest memory operation exceeds prototype limit")
        if size == 0:
            return b""
        return bytes(self.cpu.mem_read(address, size))

    def read_u64(self, address):
        return struct.unpack("<Q", self.read(address, 8))[0]

    def write_u64(self, address, value):
        self.cpu.mem_write(address, struct.pack("<Q", value & MASK))

    def arg(self, n):
        names = ("RDI", "RSI", "RDX", "RCX", "R8", "R9")
        if n >= 6:
            return self.read_u64(self.cpu.reg_read(self.x86.UC_X86_REG_RSP) + 8 * (n - 5))
        return self.cpu.reg_read(getattr(self.x86, "UC_X86_REG_" + names[n]))

    def allocate(self, size):
        if size < 0 or size > HEAP_SIZE:
            raise CompatibilityError("Guest allocation exceeds prototype limit")
        size = max(16, (size + 15) & -16)
        for i, (address, capacity) in enumerate(self.free_blocks):
            if capacity >= size:
                self.free_blocks.pop(i)
                self.allocations[address] = capacity
                return address
        if self.heap_next + size > HEAP + HEAP_SIZE:
            raise CompatibilityError("Guest heap exhausted")
        address = self.heap_next
        self.heap_next += size
        self.allocations[address] = size
        return address

    def scratch(self, data):
        address = self.scratch_next
        size = (len(data) + 15) & -16
        if address + size > SCRATCH + SCRATCH_SIZE:
            raise CompatibilityError("Guest scratch buffer exhausted")
        self.scratch_next += max(16, size)
        if data:
            self.cpu.mem_write(address, data)
        return address

    def graphics_call(self, name):
        if self.objc.registry.graphics is None:
            raise CompatibilityError('Graphics operations require the opt-in editor bridge')
        return self.objc.registry.graphics.invoke(name)

    def shim(self, name):
        self.recent_imports.append(name)
        self.import_counts[name] = self.import_counts.get(name, 0) + 1
        handler = self.shim_handlers.get(name)
        if handler is not None:
            return handler(name)
        a, b, c = self.arg(0), self.arg(1), self.arg(2)
        if name == "_srand":
            self.random_state = a & 0xffffffff
            return 0
        if name in ("_rand", "_rand_r"):
            if self.native_random is None:
                self.native_random = ctypes.CDLL("/usr/lib/libSystem.B.dylib").rand_r
                self.native_random.argtypes = [ctypes.POINTER(ctypes.c_uint32)]
                self.native_random.restype = ctypes.c_int
            value = struct.unpack("<I", self.read(a, 4))[0] if name == "_rand_r" else self.random_state
            state = ctypes.c_uint32(value)
            result = self.native_random(ctypes.byref(state))
            if name == "_rand_r":
                self.cpu.mem_write(a, struct.pack("<I", state.value))
            else:
                self.random_state = state.value
            return result
        if name == "_time":
            seconds = int(time.time())
            if a:
                self.write_u64(a, seconds)
            return seconds
        if name == "_malloc":
            return self.allocate(a)
        if name == "_calloc":
            if a * b > SCRATCH_SIZE:
                raise CompatibilityError("calloc exceeds prototype operation limit")
            address = self.allocate(a * b)
            self.cpu.mem_write(address, bytes(a * b))
            return address
        if name == "_free":
            libc_shims.release(self, a)
            return 0
        if name in ("_memcpy", "_memmove"):
            payload = self.read(b, c)
            if payload:
                self.cpu.mem_write(a, payload)
            return a
        if name == "_memset":
            if c > SCRATCH_SIZE:
                raise CompatibilityError("memset exceeds prototype operation limit")
            if c:
                self.cpu.mem_write(a, bytes([b & 255]) * c)
            return a
        if name in ("_bzero", "___bzero"):
            if b > SCRATCH_SIZE:
                raise CompatibilityError("bzero exceeds prototype operation limit")
            if b:
                self.cpu.mem_write(a, bytes(b))
            return 0
        if name == "_memchr":
            offset = self.read(a, c).find(bytes([b & 255]))
            return a + offset if offset >= 0 else 0
        if name == "_strlen":
            return len(libc_shims.string_bytes(self, a))
        raise CompatibilityError(f"Unexpected call to {name}; lazy imports should already be bound")

    def on_gate(self, cpu, address, size, user):
        try:
            if address not in self.gates:
                raise CompatibilityError("Guest called an invalid host gate")
            result = self.gates[address]()
            if isinstance(result, (darwin_shims.GuestCalls, objc_registry.TailCall)):
                self.pending_calls = result
                cpu.emu_stop()
            elif result is not None:
                cpu.reg_write(self.x86.UC_X86_REG_RAX, result & MASK)
        except Exception as exc:
            self.callback_error = exc
            cpu.emu_stop()

    def on_syscall(self, cpu, user):
        self.callback_error = CompatibilityError("Direct Darwin syscalls are not implemented")
        cpu.emu_stop()

    def on_interrupt(self, cpu, number, user):
        self.callback_error = CompatibilityError(f"Guest interrupt {number} is not implemented")
        cpu.emu_stop()

    def on_invalid_memory(self, cpu, access, address, size, value, user):
        index = (address - MISSING_BASE) // MISSING_STRIDE
        if 0 <= index < len(self.missing_imports):
            name = self.missing_imports[index]
            self.recent_imports.append(name)
            operation = "execute" if access in (self.uc.UC_MEM_FETCH_UNMAPPED, self.uc.UC_MEM_FETCH_PROT) else "access"
            if operation == "execute":
                self.missing_import_call = {"symbol": name, "integer_arguments": [self.arg(i) for i in range(6)]}
            if operation == "execute" and name in ("_objc_msgSend_fixup", "_objc_msgSend_stret_fixup"):
                # Diagnostic only: identify the legacy message without treating
                # Objective-C dispatch or any missing class as implemented.
                shift = 1 if name == "_objc_msgSend_stret_fixup" else 0
                receiver, reference = self.arg(shift), self.arg(shift + 1)
                receiver_index = (receiver - MISSING_BASE) // MISSING_STRIDE
                message = {"receiver_address": receiver, "message_reference": reference}
                if 0 <= receiver_index < len(self.missing_imports):
                    message["receiver_import"] = self.missing_imports[receiver_index]
                try:
                    selector = self.read_u64(reference + 8)
                    message["selector"] = libc_shims.string_bytes(self, selector, limit=1024).decode("utf-8", errors="replace")
                except (CompatibilityError, self.uc.UcError):
                    message["selector"] = "<unreadable>"
                self.objective_c_message = message
            self.callback_error = CompatibilityError(
                f"Unimplemented import {name}: {operation} at 0x{address:x} during {self.phase}")
        return False

    def host_callback(self):
        opcode = self.arg(1) & 0xffffffff
        self.host_calls[opcode] = self.host_calls.get(opcode, 0) + 1
        if opcode == 1:
            return 2400
        if opcode == 16:
            return int(self.sample_rate)
        if opcode == 17:
            return self.block_size
        if opcode == 23:
            return 4  # offline processing
        if opcode == 7:
            # Stopped transport, known tempo/meter. Requested validity mask is
            # intersected with the fields we actually supply.
            flags = self.arg(3) & (0x200 | 0x400 | 0x2000)
            data = struct.pack("<8d6i", self.position, self.sample_rate, 0,
                               self.position / self.sample_rate * 2, 120, 0, 0, 0,
                               4, 4, 0, 0, 0, flags)
            self.cpu.mem_write(self.time_info, data)
            return self.time_info
        if opcode in (32, 33):
            if self.arg(4):
                self.cpu.mem_write(self.arg(4), b"Cardinal VST2 Probe\0")
                return 1
            return 0
        if opcode == 34:
            return 1
        if opcode == 37:
            return 0  # no capabilities advertised beyond implemented callbacks
        self.unknown_host_calls.add(opcode)
        return 0

    def call(self, address, *args, opt=None):
        if self.call_depth >= 8:
            raise CompatibilityError("Guest callback nesting limit exceeded")
        if self.call_depth == 0:
            startup = self.phase not in ("ready", "VST process") and not self.closing
            self.call_deadline = time.monotonic() + (self.startup_seconds if startup else 2)
            self.call_instruction_limit = int(5_000_000 * max(1, self.startup_seconds / 2)) if startup else 5_000_000
            self.continuation_count = 0
        self.call_depth += 1
        try:
            return self.run_call(address, args, opt)
        finally:
            self.call_depth -= 1

    def run_call(self, address, args, opt):
        if len(args) > 16 or not address:
            raise CompatibilityError("Invalid guest call")
        regs = ("RDI", "RSI", "RDX", "RCX", "R8", "R9")
        for name in ("RAX", "RBX", "RCX", "RDX", "RSI", "RDI", "RBP", "R8", "R9", "R10", "R11", "R12", "R13", "R14", "R15"):
            self.cpu.reg_write(getattr(self.x86, "UC_X86_REG_" + name), 0)
        for name, value in zip(regs, args):
            self.cpu.reg_write(getattr(self.x86, "UC_X86_REG_" + name), value & MASK)
        self.cpu.reg_write(self.x86.UC_X86_REG_EFLAGS, 2)
        self.cpu.reg_write(self.x86.UC_X86_REG_MXCSR, 0x1f80)
        self.cpu.reg_write(self.x86.UC_X86_REG_XMM0, 0 if opt is None else struct.unpack("<I", struct.pack("<f", opt))[0])
        # SysV AMD64: RSP is 8 mod 16 on function entry, with room for red zone.
        stack = STACK + STACK_SIZE - (self.call_depth - 1) * 65536 - 8
        stack -= ((max(0, len(args) - 6) * 8 + 15) // 16) * 16
        self.write_u64(stack, RETURN)
        for index, value in enumerate(args[6:]):
            self.write_u64(stack + 8 * (index + 1), value)
        self.cpu.reg_write(self.x86.UC_X86_REG_RSP, stack)
        self.callback_error = None
        try:
            while True:
                remaining = int((self.call_deadline - time.monotonic()) * 1_000_000)
                if remaining <= 0 or self.continuation_count >= 4096:
                    raise CompatibilityError("Guest call exceeded its callback/time budget")
                self.pending_calls = None
                # Unicorn's instruction counter instruments every instruction,
                # making DSP substantially slower. Audio remains bounded by the
                # shared wall-clock deadline, callback cap, and worker IPC timeout.
                # Keep instruction limits for initialization and control calls.
                instruction_limit = 0 if self.phase == "VST process" and not self.closing else self.call_instruction_limit
                self.cpu.emu_start(address, RETURN, timeout=remaining, count=instruction_limit)
                if self.pending_calls is None:
                    break
                pending = self.pending_calls
                if isinstance(pending, objc_registry.TailCall):
                    self.continuation_count += 1
                    self.cpu.reg_write(self.x86.UC_X86_REG_RDI, pending.receiver)
                    self.cpu.reg_write(self.x86.UC_X86_REG_RSI, pending.selector)
                    address = pending.address
                    continue  # preserve argument/return registers and caller stack
                context = self.cpu.context_save()
                callbacks = iter(pending.calls)
                try:
                    for function, arguments in callbacks:
                        self.continuation_count += 1
                        self.call(function, *arguments)
                finally:
                    try:
                        if hasattr(callbacks, "close"):
                            callbacks.close()
                    finally:
                        self.cpu.context_restore(context)
                self.cpu.reg_write(self.x86.UC_X86_REG_RAX, pending.result & MASK)
                # Complete the paused shim's RET without invoking its hook again.
                rsp = self.cpu.reg_read(self.x86.UC_X86_REG_RSP)
                address = self.read_u64(rsp)
                self.cpu.reg_write(self.x86.UC_X86_REG_RSP, rsp + 8)
                if address == RETURN:
                    self.cpu.reg_write(self.x86.UC_X86_REG_RIP, RETURN)
                    break
        except self.uc.UcError as exc:
            if self.callback_error:
                raise CompatibilityError(str(self.callback_error)) from exc
            pc = self.cpu.reg_read(self.x86.UC_X86_REG_RIP)
            raise CompatibilityError(f"Intel execution failed at 0x{pc:x}: {exc}") from exc
        if self.callback_error:
            raise CompatibilityError(f"Host shim failed: {self.callback_error}")
        if self.cpu.reg_read(self.x86.UC_X86_REG_RIP) != RETURN:
            raise CompatibilityError("Guest call exceeded its time/instruction budget")
        return self.cpu.reg_read(self.x86.UC_X86_REG_RAX)

    def refresh_effect(self):
        blob = self.read(self.effect, 192)
        if struct.unpack_from("<I", blob)[0] != 0x56737450:
            raise CompatibilityError("Plugin returned an invalid AEffect magic")
        self.dispatcher, = struct.unpack_from("<Q", blob, 8)
        self.setter, self.getter = struct.unpack_from("<QQ", blob, 24)
        self.programs, self.params, self.inputs, self.outputs, self.flags = struct.unpack_from("<5i", blob, 40)
        self.processor, = struct.unpack_from("<Q", blob, 120)
        if not self.dispatcher or not self.processor or not self.flags & 16:
            raise CompatibilityError("Prototype requires a dispatcher and float processReplacing")
        if not 0 <= self.inputs <= 32 or not 1 <= self.outputs <= 32 or not 0 <= self.params <= 65536:
            raise CompatibilityError("Plugin channel/parameter count exceeds prototype limits")
        if self.params and (not self.setter or not self.getter):
            raise CompatibilityError("Plugin has parameters but no parameter callbacks")

    def dispatch(self, opcode, index=0, value=0, pointer=0, opt=0.0):
        return self.call(self.dispatcher, self.effect, opcode, index, value, pointer, opt=opt)

    def set_parameter(self, index, value):
        if not 0 <= index < self.params or not math.isfinite(value) or not 0 <= value <= 1:
            raise CompatibilityError("Invalid normalized parameter")
        self.call(self.setter, self.effect, index, opt=value)

    def get_parameter(self, index):
        if not 0 <= index < self.params:
            raise CompatibilityError("Invalid parameter index")
        self.call(self.getter, self.effect, index)
        return struct.unpack("<f", struct.pack("<I", self.cpu.reg_read(self.x86.UC_X86_REG_XMM0) & 0xffffffff))[0]

    def process(self, inputs, frames, midi=()):
        if self.trace_imports and self.missing_imports:
            raise CompatibilityError("Diagnostic import tracing cannot establish audio compatibility; implement remaining imports first")
        return self._process(inputs, frames, midi)

    def diagnose_audio(self, inputs, frames, midi=()):
        """Explicit offline diagnostic; unresolved imports still trap on first use.

        Success establishes only that this input exercised a working audio path,
        not that the plugin's unexercised imports or interface are supported.
        """
        if not self.trace_imports:
            raise CompatibilityError("Audio tracing requires diagnostic import guards")
        return self._process(inputs, frames, midi)

    def _process(self, inputs, frames, midi):
        if not self.active or not 1 <= frames <= self.block_size:
            raise CompatibilityError("Invalid processing state or frame count")
        if len(inputs) != self.inputs or any(len(ch) != frames for ch in inputs):
            raise CompatibilityError("Input audio does not match plugin channel/frame count")
        if len(midi) > 256:
            raise CompatibilityError("Too many MIDI events in one block")
        previous_phase = self.phase
        self.phase = "VST process"
        self.scratch_next = SCRATCH
        if midi:
            events = []
            for delta, status, data1, data2 in midi:
                if not 0 <= delta < frames or not 0x80 <= status <= 0xef or not 0 <= data1 < 128 or not 0 <= data2 < 128:
                    raise CompatibilityError("Invalid MIDI channel event")
                events.append(self.scratch(struct.pack("<6i8B", 1, 32, delta, 0, 0, 0, status, data1, data2, 0, 0, 0, 0, 0)))
            header = struct.pack("<i4xQ", len(events), 0) + struct.pack(f"<{len(events)}Q", *events)
            self.dispatch(25, pointer=self.scratch(header))
        ins = [self.scratch(struct.pack(f"<{frames}f", *ch)) for ch in inputs]
        outs = [self.scratch(bytes(frames * 4)) for _ in range(self.outputs)]
        in_table = self.scratch(struct.pack(f"<{len(ins)}Q", *ins))
        out_table = self.scratch(struct.pack(f"<{len(outs)}Q", *outs))
        self.call(self.processor, self.effect, in_table, out_table, frames)
        result = [list(struct.unpack(f"<{frames}f", self.read(ch, frames * 4))) for ch in outs]
        if any(not math.isfinite(v) for ch in result for v in ch):
            raise CompatibilityError("Plugin produced non-finite audio")
        self.position += frames
        self.phase = previous_phase
        return result

    def close(self, suppress=False):
        self.closing = True
        first_error = None
        for attr, opcode, value in (("started", 72, 0), ("active", 12, 0), ("opened", 1, 0)):
            if getattr(self, attr):
                setattr(self, attr, False)
                try:
                    self.dispatch(opcode, value=value)
                except Exception as exc:
                    first_error = first_error or exc
        self.effect = 0
        if not self.runtime_finalized:
            self.runtime_finalized = True
            try:
                if self.darwin.exit_handlers:
                    self.call(self.shim_address("___cxa_finalize"), 0)
            except Exception as exc:
                first_error = first_error or exc
        for bridge in (self.cf, self.objc, self.stdio, self.filesystem, self.darwin):
            try:
                bridge.close()
            except Exception as exc:
                first_error = first_error or exc
        if first_error and not suppress:
            raise first_error
