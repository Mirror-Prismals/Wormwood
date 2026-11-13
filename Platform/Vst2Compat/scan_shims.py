"""Bounded fscanf subset using guest output addresses and AMD64 varargs."""
import re
from format_shims import Arguments
from libc_shims import string_bytes
from macho import CompatibilityError

SPACE = b' \t\r\n\v\f'
LIMIT = 65536


def scan(stdio, pointer, format_pointer):
    w = stdio.worker
    fd = stdio.stream(pointer)
    fmt = string_bytes(w, format_pointer, limit=4096)
    # Validate the entire format before changing file position or guest memory.
    parts = []
    pos = 0
    while pos < len(fmt):
        char = fmt[pos:pos + 1]
        if char != b'%':
            parts.append(char)
            pos += 1
            continue
        if fmt[pos:pos + 2] == b'%%':
            parts.append(b'%')
            pos += 2
            continue
        match = re.match(rb'%([*]?)([0-9]*)(hh|ll|h|l)?([sduc])', fmt[pos:])
        if not match:
            raise CompatibilityError('Unsupported fscanf conversion')
        suppress, width, length, kind = match.groups()
        width = int(width) if width else None
        if width is not None and not 1 <= width < LIMIT or length and kind in (b's', b'c'):
            raise CompatibilityError('Unsupported fscanf width or wide-character conversion')
        parts.append((bool(suppress), width, length, kind))
        pos += len(match[0])
    arguments = Arguments(w, 2)
    pending = stdio.pushback if fd == 0 else stdio.file_pushback.get(fd)
    if pending is None:
        raise CompatibilityError('fscanf requires a readable stream')
    consumed = 0
    assigned = 0
    def peek():
        if pending:
            return bytes((pending[-1],))
        data = stdio.read_file(fd, 1)
        if data:
            pending.append(data[0])
        else:
            stdio.flags(pointer, set_bits=0x20)
        return data
    def take():
        nonlocal consumed
        data = peek()
        if data:
            pending.pop()
            consumed += 1
            if consumed > LIMIT:
                raise CompatibilityError('fscanf input exceeds operation limit')
        return data
    def spaces():
        while (value := peek()) and value in SPACE:
            take()
    for part in parts:
        if isinstance(part, bytes):
            if part in SPACE:
                spaces()
            elif peek() != part:
                return assigned if assigned or peek() else -1
            else:
                take()
            continue
        suppress, width, length, kind = part
        if kind != b'c':
            spaces()
        if not peek():
            return assigned if assigned else -1
        value = bytearray()
        maximum = width or (1 if kind == b'c' else LIMIT)
        if kind in (b'd', b'u'):
            if peek() in (b'+', b'-'):
                value.extend(take())
            while len(value) < maximum and (digit := peek()) and b'0' <= digit <= b'9':
                value.extend(take())
            if not value or value in (b'+', b'-'):
                return assigned
            size = {None: 4, b'hh': 1, b'h': 2, b'l': 8, b'll': 8}[length]
            number = int(value)
            data = (number & ((1 << (size * 8)) - 1)).to_bytes(size, 'little')
        else:
            while len(value) < maximum and (char := peek()) and (kind == b'c' or char not in SPACE):
                value.extend(take())
            if kind == b'c' and len(value) != maximum:
                return assigned if assigned else -1
            data = bytes(value) + (b'\0' if kind == b's' else b'')
        if len(value) >= LIMIT:
            raise CompatibilityError('fscanf field exceeds operation limit')
        if not suppress:
            w.cpu.mem_write(arguments.next(), data)
            assigned += 1
    return assigned
