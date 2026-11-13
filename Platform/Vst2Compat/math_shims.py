"""Explicit scalar libm bridge from Intel XMM arguments to native ARM functions."""
import ctypes
import struct

ARITY = {name: 1 for name in ("sin", "cos", "tan", "asin", "acos", "atan",
                              "sinh", "cosh", "tanh", "exp", "log", "log10", "log2",
                              "sqrt", "floor", "ceil", "fabs")}
ARITY.update(atan2=2, pow=2, fmod=2)
SIGNATURES = {"_" + name + suffix: (arity, suffix == "f")
              for name, arity in ARITY.items() for suffix in ("", "f")}
SYMBOLS = tuple(SIGNATURES)


class Math:
    def __init__(self, worker):
        self.worker = worker
        self.library, self.functions = None, {}

    def prepare(self, name):
        w = self.worker
        arity, single = SIGNATURES[name]
        if self.library is None:
            self.library = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
        function = getattr(self.library, name[1:])
        scalar = ctypes.c_float if single else ctypes.c_double
        function.argtypes, function.restype = [scalar] * arity, scalar
        floating = struct.Struct("<f" if single else "<d")
        integer = struct.Struct("<I" if single else "<Q")
        mask = (1 << (32 if single else 64)) - 1
        read, write = w.cpu.reg_read, w.cpu.reg_write
        xmm0, xmm1 = w.x86.UC_X86_REG_XMM0, w.x86.UC_X86_REG_XMM1
        unpack_float, pack_float = floating.unpack, floating.pack
        unpack_int, pack_int = integer.unpack, integer.pack

        def invoke():
            first = unpack_float(pack_int(read(xmm0) & mask))[0]
            if arity == 2:
                second = unpack_float(pack_int(read(xmm1) & mask))[0]
            ctypes.set_errno(0)
            value = function(first) if arity == 1 else function(first, second)
            error = ctypes.get_errno()
            if error:
                w.stdio.errno(error)
            write(xmm0, unpack_int(pack_float(value))[0])
            # SysV floating returns live in XMM0, not RAX. Avoid a second
            # generic register conversion/write for a nonexistent integer result.
            return None

        self.functions[name] = invoke
        return invoke

    def invoke(self, name):
        function = self.functions.get(name)
        if function is None:
            function = self.prepare(name)
        return function()
