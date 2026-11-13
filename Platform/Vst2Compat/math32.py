"""Native scalar libm with Darwin i386 stack arguments and x87 returns."""
import ctypes
import struct

from math_shims import SIGNATURES, SYMBOLS


class Math32:
    def __init__(self, worker, result_address):
        self.worker, self.result_address = worker, result_address
        self.library = None
        self.functions = {}

    def prepare(self, name):
        w = self.worker
        arity, single = SIGNATURES[name]
        if self.library is None:
            self.library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
        function = getattr(self.library, name[1:])
        scalar = ctypes.c_float if single else ctypes.c_double
        function.argtypes, function.restype = [scalar] * arity, scalar
        arguments = struct.Struct('<' + ('f' if single else 'd') * arity)
        pack = struct.Struct('<d').pack
        esp = w.x86.UC_X86_REG_ESP
        def invoke():
            address = w.cpu.reg_read(esp) + 4
            values = arguments.unpack(w.read(address, arguments.size))
            ctypes.set_errno(0)
            value = function(*values)
            error = ctypes.get_errno()
            if error:
                w.stdio.errno(error)
            # The guest gate executes FLD m64fp; RET, which pushes the exact
            # float/double result onto ST(0) and updates x87 tags/top correctly.
            w.cpu.mem_write(self.result_address, pack(value))
        self.functions[name] = invoke
        return invoke

    def invoke(self, name):
        function = self.functions.get(name)
        if function is None:
            function = self.prepare(name)
        function()
