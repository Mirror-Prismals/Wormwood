"""Bounded printf string formatting using the guest AMD64 variadic ABI."""
import struct
from macho import CompatibilityError
from libc_shims import string_bytes

SYMBOLS = ("_sprintf", "_snprintf", "_vsprintf", "_vsnprintf")
LIMIT = 65536


class Arguments:
    long_bits = pointer_bits = 64
    def __init__(self, worker, fixed, va_list=0):
        self.w, self.gp, self.fp = worker, fixed, 0
        self.stack = worker.cpu.reg_read(worker.x86.UC_X86_REG_RSP) + 8
        self.saved = None
        if va_list:
            gp, fp, self.stack, self.saved = struct.unpack("<IIQQ", worker.read(va_list, 24))
            if gp > 48 or gp % 8 or fp < 48 or fp > 176 or (fp - 48) % 16:
                raise CompatibilityError("Invalid guest va_list offsets")
            self.gp, self.fp = gp // 8, (fp - 48) // 16

    def next(self, floating=False, bits=32):
        w = self.w
        if floating and self.fp < 8:
            value = w.read_u64(self.saved + 48 + self.fp * 16) if self.saved is not None else (
                w.cpu.reg_read(getattr(w.x86, f"UC_X86_REG_XMM{self.fp}")) & ((1 << 64) - 1))
            self.fp += 1
        elif not floating and self.gp < 6:
            value = w.read_u64(self.saved + self.gp * 8) if self.saved is not None else w.arg(self.gp)
            self.gp += 1
        else:
            value = w.read_u64(self.stack)
            self.stack += 8
        return struct.unpack("<d", struct.pack("<Q", value))[0] if floating else value


def signed(value, bits=32):
    value &= (1 << bits) - 1
    return value - (1 << bits) if value & (1 << (bits - 1)) else value


def format_bytes(worker, address, args):
    fmt = string_bytes(worker, address, limit=LIMIT)
    result = bytearray()
    i = 0
    while i < len(fmt):
        if fmt[i] != 37:
            result.append(fmt[i]); i += 1
            continue
        i += 1
        if i < len(fmt) and fmt[i] == 37:
            result.append(37); i += 1
            continue
        flags = ""
        while i < len(fmt) and chr(fmt[i]) in "-+ #0":
            flags += chr(fmt[i]); i += 1
        width, precision = 0, None
        if i < len(fmt) and fmt[i] == 42:
            width = signed(args.next()); i += 1
            if width < 0:
                flags += "-"; width = -width
        else:
            while i < len(fmt) and 48 <= fmt[i] <= 57:
                width = width * 10 + fmt[i] - 48; i += 1
                if width > LIMIT:
                    raise CompatibilityError("printf width exceeds limit")
        if i < len(fmt) and fmt[i] == 46:
            i += 1; precision = 0
            if i < len(fmt) and fmt[i] == 42:
                precision = signed(args.next()); i += 1
                if precision < 0:
                    precision = None
            else:
                while i < len(fmt) and 48 <= fmt[i] <= 57:
                    precision = precision * 10 + fmt[i] - 48; i += 1
                    if precision > LIMIT:
                        raise CompatibilityError("printf precision exceeds limit")
        if width > LIMIT or precision is not None and precision > LIMIT:
            raise CompatibilityError("printf field exceeds limit")
        length = ""
        for candidate in (b"hh", b"ll", b"h", b"l", b"j", b"z", b"t", b"L"):
            if fmt[i:i + len(candidate)] == candidate:
                length = candidate.decode(); i += len(candidate); break
        if i == len(fmt):
            raise CompatibilityError("Incomplete printf conversion")
        kind = chr(fmt[i]); i += 1
        prefix, numeric = b"", False
        if kind in "diuoxXp":
            if length == "L":
                raise CompatibilityError("Invalid integer printf length")
            bits = (args.pointer_bits if kind == "p" else 64 if length in ("ll", "j") else
                    args.long_bits if length in ("l", "z", "t") else 8 if length == "hh" else 16 if length == "h" else 32)
            number = args.next(bits=bits) & ((1 << bits) - 1)
            if kind in "di":
                number = signed(number, bits)
                prefix = b"-" if number < 0 else b"+" if "+" in flags else b" " if " " in flags else b""
                number = abs(number)
            digits = format(number, "x" if kind == "p" else kind if kind in "oxX" else "d").encode()
            if precision == 0 and number == 0 and kind != "p":
                digits = b""
            if precision is not None:
                digits = digits.rjust(precision, b"0")
            if kind == "p" or "#" in flags and kind in "xX" and number:
                prefix += b"0X" if kind == "X" else b"0x"
            if "#" in flags and kind == "o" and not digits.startswith(b"0"):
                prefix += b"0"
            payload, numeric = digits, True
        elif kind == "s" and not length:
            pointer = args.next()
            payload = string_bytes(worker, pointer, limit=LIMIT if precision is None else precision,
                                   require_null=precision is None) if pointer else b"(null)"
            if precision is not None:
                payload = payload[:precision]
        elif kind == "c" and not length:
            payload = bytes([args.next() & 255])
        elif kind in "fFeEgG" and length in ("", "l"):
            # Python's percent formatter handles numeric padding/rounding for
            # this explicit subset. Long double and positional formats reject.
            spec = "%" + "".join(flag for flag in "-+ #0" if flag in flags)
            spec += str(width) if width else ""
            if precision is not None:
                spec += "." + str(precision)
            payload = ((spec + kind) % args.next(floating=True)).encode("ascii")
            width = 0
        else:
            raise CompatibilityError(f"Unsupported printf conversion %{length}{kind}")
        padding = max(0, width - len(prefix) - len(payload))
        if numeric and "0" in flags and "-" not in flags and precision is None:
            payload = prefix + b"0" * padding + payload
        else:
            payload = prefix + payload
            payload = payload + b" " * padding if "-" in flags else b" " * padding + payload
        if len(result) + len(payload) > LIMIT:
            raise CompatibilityError("printf output exceeds limit")
        result.extend(payload)
    if len(result) > LIMIT:
        raise CompatibilityError("printf output exceeds limit")
    return bytes(result)


def invoke(worker, name, arguments_type=Arguments):
    destination = worker.arg(0)
    bounded = name in ("_snprintf", "_vsnprintf")
    fixed = 3 if bounded else 2
    size = worker.arg(1) if bounded else LIMIT + 1
    fmt = worker.arg(fixed - 1)
    va_list = worker.arg(fixed) if name.startswith("_v") else 0
    if name.startswith("_v") and not va_list:
        raise CompatibilityError("Null printf va_list")
    result = format_bytes(worker, fmt, arguments_type(worker, fixed, va_list))
    if size:
        worker.cpu.mem_write(destination, result[:size - 1] + b"\0")
    return len(result)
