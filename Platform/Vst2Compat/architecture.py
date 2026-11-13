"""Select an Intel guest ABI from bounded Mach-O headers, without execution."""
import struct
from macho import CompatibilityError, MAX_IMAGE, checked, resolve_binary, unpack


def choose_architecture(header, file_size):
    magic = checked(header, 0, 4)
    formats = {b'\xca\xfe\xba\xbe': ('>', False), b'\xbe\xba\xfe\xca': ('<', False),
               b'\xca\xfe\xba\xbf': ('>', True), b'\xbf\xba\xfe\xca': ('<', True)}
    if magic not in formats:
        cpu, = unpack('<I', header, 4)
        if magic == b'\xcf\xfa\xed\xfe' and cpu == 0x01000007:
            return 'x86_64'
        if magic == b'\xce\xfa\xed\xfe' and cpu == 7:
            return 'i386'
        raise CompatibilityError('Plugin has no supported Intel Mac architecture')
    endian, wide = formats[magic]
    count, = unpack(endian + 'I', header, 4)
    if count > 64:
        raise CompatibilityError('Too many universal binary slices')
    fmt = endian + ('IIQQII' if wide else 'IIIII')
    stride = struct.calcsize(fmt)
    checked(header, 8, count * stride)
    cpus = set()
    for index in range(count):
        cpu, _, offset, size, *_ = unpack(fmt, header, 8 + index * stride)
        if offset < 8 + count * stride or size < 8 or offset > file_size or size > file_size - offset:
            raise CompatibilityError('Invalid universal binary slice bounds')
        cpus.add(cpu)
    if 0x01000007 in cpus:
        return 'x86_64'
    if 7 in cpus:
        return 'i386'
    raise CompatibilityError('Plugin has no supported Intel Mac architecture')


def detect_architecture(path):
    binary = resolve_binary(path)
    size = binary.stat().st_size
    if size > MAX_IMAGE:
        raise CompatibilityError('Oversized plugin binary')
    with binary.open('rb') as stream:
        return choose_architecture(stream.read(8 + 64 * 32), size)
