"""Darwin random sequences with per-worker state and four-byte guest seeds."""
import ctypes
import struct

SYMBOLS = ('_rand', '_srand', '_rand_r')


class Random32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.state = 1
        self.function = None

    def invoke(self, name, arg):
        if name == '_srand':
            self.state = arg(0)
            return 0
        if self.function is None:
            self.function = ctypes.CDLL('/usr/lib/libSystem.B.dylib').rand_r
            self.function.argtypes = [ctypes.POINTER(ctypes.c_uint32)]
            self.function.restype = ctypes.c_int
        address = arg(0) if name == '_rand_r' else None
        seed = self.runtime.read_u32(address) if address is not None else self.state
        state = ctypes.c_uint32(seed)
        result = self.function(ctypes.byref(state))
        if address is None:
            self.state = state.value
        else:
            self.runtime.cpu.mem_write(address, struct.pack('<I', state.value))
        return result
