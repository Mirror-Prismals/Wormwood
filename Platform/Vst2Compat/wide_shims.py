"""Bounded 32-bit Darwin wchar_t operations in the default NONE/C encoding."""
import struct

from macho import CompatibilityError
from libc_shims import string_bytes

SYMBOLS = ('_mbstowcs', '_wcstombs', '_wcslen', '_wcscpy', '_wcsncpy', '_wcsncat', '_wcsncmp',
           '_toupper', '___toupper', '_tolower', '___tolower')
LIMIT = 65536


def wide(worker, address, limit=LIMIT, require_null=True):
    if not 0 <= limit <= LIMIT:
        raise CompatibilityError('Wide string exceeds guest operation limit')
    values = []
    for index in range(limit):
        value, = struct.unpack('<I', worker.read(address + index * 4, 4))
        if not value:
            return values
        values.append(value)
    if require_null:
        raise CompatibilityError('Unterminated guest wide string')
    return values


def invoke(worker, name):
    a, b, c = (worker.arg(index) for index in range(3))
    if name in ('_toupper', '___toupper'):
        return a - 32 if 97 <= a <= 122 else a
    if name in ('_tolower', '___tolower'):
        return a + 32 if 65 <= a <= 90 else a
    if name == '_wcslen':
        return len(wide(worker, a))
    if name == '_mbstowcs':
        values = string_bytes(worker, b, limit=min(c, LIMIT) if a else LIMIT, require_null=not a)
        if a:
            if c > LIMIT:
                raise CompatibilityError('Wide conversion exceeds guest operation limit')
            result = list(values) + ([0] if len(values) < c else [])
            if result:
                worker.cpu.mem_write(a, struct.pack('<' + 'I' * len(result), *result))
        return len(values)
    if name == '_wcstombs':
        values = wide(worker, b, min(c, LIMIT) if a else LIMIT, require_null=not a)
        if any(value > 255 for value in values):
            worker.stdio.errno(92)  # Darwin EILSEQ; default NONE encoding
            return -1
        if a:
            if c > LIMIT:
                raise CompatibilityError('Wide conversion exceeds guest operation limit')
            result = bytes(values) + (b'\0' if len(values) < c else b'')
            if result:
                worker.cpu.mem_write(a, result)
        return len(values)
    if name == '_wcsncmp':
        left, right = wide(worker, a, c, False), wide(worker, b, c, False)
        return (left > right) - (left < right)
    values = wide(worker, b, c if name in ('_wcsncpy', '_wcsncat') else LIMIT,
                  require_null=name == '_wcscpy')
    if name == '_wcsncpy':
        values += [0] * (c - len(values))
    else:
        values.append(0)
    offset = len(wide(worker, a)) * 4 if name == '_wcsncat' else 0
    if values:
        worker.cpu.mem_write(a + offset, struct.pack('<' + 'I' * len(values), *values))
    return a
