"""Darwin i386 variadic stack reader for the shared bounded printf formatter."""
import struct


class Arguments32:
    long_bits = pointer_bits = 32

    def __init__(self, worker, fixed, va_list=0):
        self.worker = worker
        self.stack = va_list or (worker.cpu.reg_read(worker.x86.UC_X86_REG_ESP) + 4 + fixed * 4)

    def next(self, floating=False, bits=32):
        size = 8 if floating or bits == 64 else 4
        data = self.worker.read(self.stack, size)
        self.stack += size  # i386 stack varargs have four-byte alignment
        return struct.unpack('<d', data)[0] if floating else int.from_bytes(data, 'little')
