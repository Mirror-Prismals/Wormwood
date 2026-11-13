"""Small bounded C library operations over guest memory, never host pointers."""
from macho import CompatibilityError

SYMBOLS = ("_strcpy", "_strncpy", "_strcat", "_strcmp", "_strncmp", "_strchr",
           "_strrchr", "_strstr", "_memcmp", "_strdup", "_realloc",
           "__Znwm", "__Znam", "__ZdlPv", "__ZdaPv",
           "__ZN9__gnu_cxx18__exchange_and_addEPVii")
MAX_COPY = 4 * 1024 * 1024


def string_bytes(worker, address, limit=65536, require_null=True):
    if not 0 <= limit <= MAX_COPY:
        raise CompatibilityError("String operation exceeds prototype limit")
    result = bytearray()
    while len(result) < limit:
        start = address + len(result)
        size = min(limit - len(result), 4096 - start % 4096)
        part = worker.read(start, size)
        end = part.find(b"\0")
        if end >= 0:
            result.extend(part[:end])
            return bytes(result)
        result.extend(part)
    if require_null:
        raise CompatibilityError("Unterminated or oversized guest string")
    return bytes(result)


def release(worker, address):
    if not address:
        return
    if address not in worker.allocations or address == worker.time_info:
        raise CompatibilityError("Invalid guest free/delete")
    worker.free_blocks.append((address, worker.allocations.pop(address)))


def invoke(worker, name):
    a, b, c = worker.arg(0), worker.arg(1), worker.arg(2)
    if name in ("__Znwm", "__Znam"):
        return worker.allocate(a)
    if name in ("__ZdlPv", "__ZdaPv"):
        release(worker, a)
        return 0
    if name == "__ZN9__gnu_cxx18__exchange_and_addEPVii":
        # The worker is single-threaded; no guest threads are supported.
        old = int.from_bytes(worker.read(a, 4), "little")
        worker.cpu.mem_write(a, ((old + b) & 0xffffffff).to_bytes(4, "little"))
        return old
    if name == "_realloc":
        if a and a not in worker.allocations:
            raise CompatibilityError("Invalid guest realloc")
        if b > MAX_COPY:
            raise CompatibilityError("realloc exceeds prototype copy limit")
        result = worker.allocate(b)
        if a:
            payload = worker.read(a, min(b, worker.allocations[a]))
            if payload:
                worker.cpu.mem_write(result, payload)
            release(worker, a)
        return result
    if name == "_memcmp":
        left, right = worker.read(a, c), worker.read(b, c)
        return (left > right) - (left < right)
    if name in ("_strcmp", "_strncmp"):
        limit = c if name == "_strncmp" else 65536
        left = string_bytes(worker, a, limit, name == "_strcmp")
        right = string_bytes(worker, b, limit, name == "_strcmp")
        return (left > right) - (left < right)
    if name in ("_strcpy", "_strcat", "_strncpy", "_strdup"):
        if name == "_strncpy":
            payload = string_bytes(worker, b, c, False).ljust(c, b"\0")
            if payload:
                worker.cpu.mem_write(a, payload)
            return a
        source = string_bytes(worker, a if name == "_strdup" else b) + b"\0"
        target = worker.allocate(len(source)) if name == "_strdup" else a
        offset = len(string_bytes(worker, a)) if name == "_strcat" else 0
        worker.cpu.mem_write(target + offset, source)
        return target
    source = string_bytes(worker, a)
    if name in ("_strchr", "_strrchr"):
        source += b"\0"
        needle = bytes([b & 255])
        index = source.find(needle) if name == "_strchr" else source.rfind(needle)
    elif name == "_strstr":
        index = source.find(string_bytes(worker, b))
    else:
        raise CompatibilityError(f"Unimplemented C shim {name}")
    return 0 if index < 0 else a + index
