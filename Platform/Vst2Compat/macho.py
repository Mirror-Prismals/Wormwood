"""Bounded, read-only Mach-O inspection for the VST2 compatibility worker.

This module never asks macOS to load the inspected code. It also works without
Unicorn, so a real plugin can be inspected before its dependencies are supported.
"""
from dataclasses import dataclass
from pathlib import Path
import plistlib
import struct

MAX_IMAGE = 256 * 1024 * 1024
X86_64 = 0x01000007


class CompatibilityError(Exception):
    pass


def checked(data, offset, size):
    if offset < 0 or size < 0 or offset > len(data) or size > len(data) - offset:
        raise CompatibilityError("Truncated or out-of-range Mach-O data")
    return data[offset:offset + size]


def unpack(fmt, data, offset=0):
    return struct.unpack(fmt, checked(data, offset, struct.calcsize(fmt)))


def cstring(data, offset=0):
    checked(data, offset, 1)
    end = data.find(b"\0", offset)
    if end < 0:
        raise CompatibilityError("Unterminated Mach-O string")
    return data[offset:end].decode("utf-8", errors="replace")


def resolve_binary(path):
    path = Path(path).resolve()
    if not path.is_dir():
        return path
    info = path / "Contents/Info.plist"
    if info.stat().st_size > 1024 * 1024:
        raise CompatibilityError("Oversized bundle Info.plist")
    with info.open("rb") as stream:
        name = plistlib.load(stream).get("CFBundleExecutable")
    if not isinstance(name, str) or not name or Path(name).name != name:
        raise CompatibilityError("Bundle has no valid CFBundleExecutable")
    binary = (path / "Contents/MacOS" / name).resolve()
    if not binary.is_relative_to(path):
        raise CompatibilityError("Bundle executable escapes its bundle")
    return binary


def intel_slice(data):
    magic = checked(data, 0, 4)
    # Universal containers can contain Intel and ARM code. Always select Intel.
    formats = {b"\xca\xfe\xba\xbe": (">", False),
               b"\xbe\xba\xfe\xca": ("<", False),
               b"\xca\xfe\xba\xbf": (">", True),
               b"\xbf\xba\xfe\xca": ("<", True)}
    if magic not in formats:
        return data
    endian, wide = formats[magic]
    count, = unpack(endian + "I", data, 4)
    if count > 64:
        raise CompatibilityError("Too many universal binary slices")
    fmt = endian + ("IIQQII" if wide else "IIIII")
    stride = struct.calcsize(fmt)
    checked(data, 8, count * stride)
    for i in range(count):
        cpu, subtype, offset, size, *_ = unpack(fmt, data, 8 + i * stride)
        if cpu == X86_64:
            return checked(data, offset, size)
    raise CompatibilityError("No Intel x86_64 slice (32-bit/ARM-only plugins are not supported yet)")


@dataclass
class Segment:
    name: str
    address: int
    size: int
    offset: int
    file_size: int
    protection: int


class MachO:
    def __init__(self, data):
        self.data = intel_slice(data)
        magic, cpu, subtype, kind, count, size, flags, reserved = unpack("<8I", self.data)
        if magic != 0xfeedfacf or cpu != X86_64:
            raise CompatibilityError("Expected a little-endian Intel x86_64 Mach-O plugin")
        if kind not in (6, 8):  # MH_DYLIB, MH_BUNDLE
            raise CompatibilityError("Expected MH_BUNDLE or MH_DYLIB, not an executable/object")
        self.kind = kind
        if count > 4096:
            raise CompatibilityError("Too many Mach-O load commands")
        checked(self.data, 32, size)
        self.segments, self.dependencies, self.initializers = [], [], []
        self.symbols, self.imports, self.unsupported = {}, [], []
        self.exports, self.weak_definitions = {}, set()
        self.bind_streams = []
        self.weak_streams = []
        self.symbol_table, self.pointer_sections = [], []
        self.legacy_bindings = []
        self.local_relocation_count = 0
        self.has_dyld_info = False
        self.dysymtab = None
        if flags & 0x10:  # MH_PREBOUND needs undoing library-specific bindings
            self.unsupported.append("prebound image")
        symtab = None
        pos, end = 32, 32 + size
        for _ in range(count):
            cmd, length = unpack("<II", self.data, pos)
            if length < 8 or length % 8 or pos + length > end:
                raise CompatibilityError("Invalid Mach-O load command size")
            blob = checked(self.data, pos, length)
            if cmd == 0x19:  # LC_SEGMENT_64
                _, _, name, addr, memsize, off, nbytes, maxprot, prot, nsects, segflags = unpack("<II16sQQQQiiII", blob)
                if memsize > MAX_IMAGE or nbytes > memsize or addr + memsize > (1 << 40):
                    raise CompatibilityError("Segment exceeds guest image limits")
                checked(self.data, off, nbytes)
                checked(blob, 72, nsects * 80)
                self.segments.append(Segment(name.rstrip(b"\0").decode("ascii", errors="replace"), addr, memsize, off, nbytes, prot & 7))
                for n in range(nsects):
                    section = unpack("<16s16sQQIIIIIIII", blob, 72 + n * 80)
                    secaddr, secsize, secflags = section[2], section[3], section[8]
                    if secaddr < addr or secaddr + secsize > addr + memsize:
                        raise CompatibilityError("Section outside its segment")
                    if section[7]:
                        self.unsupported.append("section relocations")
                    if (secflags & 0xff) in (6, 7):
                        if secsize % 8:
                            raise CompatibilityError("Invalid indirect pointer section size")
                        self.pointer_sections.append((secaddr, secsize, section[9]))
                    if secflags & 0xff == 9:  # S_MOD_INIT_FUNC_POINTERS
                        if secsize % 8:
                            raise CompatibilityError("Invalid initializer section")
                        self.initializers.extend(range(secaddr, secaddr + secsize, 8))
                    if 0x11 <= (secflags & 0xff) <= 0x15:
                        self.unsupported.append("thread-local storage")
            elif cmd == 2:
                _, _, *symtab = unpack("<6I", blob)
            elif cmd in (0xc, 0x80000018, 0x8000001f, 0x80000023):
                nameoff, = unpack("<I", blob, 8)
                if nameoff < 24:
                    raise CompatibilityError("Invalid dependency name offset")
                self.dependencies.append(cstring(blob, nameoff))
            elif cmd in (0x22, 0x80000022):  # LC_DYLD_INFO[_ONLY]
                self.has_dyld_info = True
                fields = unpack("<12I", blob)
                for off, length in zip(fields[2::2], fields[3::2]):
                    checked(self.data, off, length)
                if fields[7]:
                    self.weak_streams.append((checked(self.data, fields[6], fields[7]), False))
                self.bind_streams.extend([(checked(self.data, fields[4], fields[5]), False),
                                          (checked(self.data, fields[8], fields[9]), True)])
            elif cmd == 0x80000034:
                self.unsupported.append("chained fixups")
            elif cmd == 0xb:  # LC_DYSYMTAB
                if self.dysymtab is not None:
                    raise CompatibilityError("Duplicate dynamic symbol table")
                self.dysymtab = unpack("<20I", blob)
            pos += len(blob)
        if pos != end:
            raise CompatibilityError("Load command count does not match command bytes")
        ranges = sorted((s.address, s.address + s.size) for s in self.segments if s.size)
        if sum(s.size for s in self.segments) > MAX_IMAGE:
            raise CompatibilityError("Image exceeds guest memory limit")
        if any(a[1] > b[0] for a, b in zip(ranges, ranges[1:])):
            raise CompatibilityError("Overlapping Mach-O segments")
        if symtab:
            off, count, stroff, strsize = symtab
            checked(self.data, off, count * 16)
            strings = checked(self.data, stroff, strsize)
            for i in range(count):
                nameoff, typ, sect, desc, value = unpack("<IBBHQ", self.data, off + i * 16)
                name = cstring(strings, nameoff) if nameoff else ""
                # Keep original indices, including unnamed/debug symbols: legacy
                # relocations index nlist entries, not our exported-name map.
                self.symbol_table.append((name, typ, value))
                if typ & 0xe0 or not nameoff:
                    continue
                if typ & 0xe == 0:
                    self.imports.append(name)
                elif (typ & 0xe) in (2, 0xe):  # absolute or section-defined
                    self.symbols[name] = value
                    if typ & 1 and not typ & 0x10:  # external, not private
                        self.exports[name] = value
                        if desc & 0x80:  # N_WEAK_DEF
                            self.weak_definitions.add(name)
        if self.dysymtab is not None:
            self.parse_legacy_tables()
        self.entry = next((self.symbols[n] for n in ("_VSTPluginMain", "_main_macho", "_main") if n in self.symbols), None)

    def segment_at(self, address, size):
        for segment in self.segments:
            if segment.address <= address and address + size <= segment.address + segment.size:
                return segment
        raise CompatibilityError("Relocation target outside mapped image")

    def pointer_at(self, address):
        segment = self.segment_at(address, 8)
        offset = address - segment.address
        if offset >= segment.file_size:
            return 0  # zero-filled guest memory
        available = min(8, segment.file_size - offset)
        value = checked(self.data, segment.offset + offset, available)
        return int.from_bytes(value.ljust(8, b"\0"), "little")

    def symbol_name(self, index):
        if index >= len(self.symbol_table):
            raise CompatibilityError("Relocation symbol index outside symbol table")
        name, typ, value = self.symbol_table[index]
        if not name or typ & 0xe0 or (typ & 0xe) not in (0, 2, 0xe):
            raise CompatibilityError("Unsupported relocation symbol")
        if (typ & 0xe) == 0 and value:
            raise CompatibilityError("Common-symbol allocation is not implemented")
        return name

    def parse_legacy_tables(self):
        """Legacy x86_64 dynamic pointer relocations at zero image slide.

        Unlike 32-bit Mach-O, the relocation base is the first writable segment
        even without MH_SPLIT_SEGS. External addends are stored in the pointer.
        No native dyld entry point or library is invoked.
        """
        values = self.dysymtab
        indirect_off, indirect_count, ext_off, ext_count, loc_off, loc_count = values[14:20]
        if indirect_count + ext_count + loc_count > 1_000_000:
            raise CompatibilityError("Too many legacy dynamic relocation entries")
        checked(self.data, indirect_off, indirect_count * 4)
        checked(self.data, ext_off, ext_count * 8)
        checked(self.data, loc_off, loc_count * 8)
        writable = next((s for s in self.segments if s.protection & 2), None)
        if (ext_count or loc_count) and writable is None:
            raise CompatibilityError("Legacy relocations have no writable base segment")
        for off, count, external in ((loc_off, loc_count, False), (ext_off, ext_count, True)):
            for i in range(count):
                relative, bits = unpack("<iI", self.data, off + i * 8)
                symbol_index = bits & 0xffffff
                pcrel, width, is_external, kind = (bits >> 24) & 1, (bits >> 25) & 3, (bits >> 27) & 1, bits >> 28
                if pcrel or width != 3 or is_external != external or kind != 0:
                    raise CompatibilityError("Unsupported legacy x86_64 relocation (requires unsigned 64-bit pointer)")
                address = writable.address + relative
                self.segment_at(address, 8)
                if external:
                    self.legacy_bindings.append((address, self.symbol_name(symbol_index), self.pointer_at(address)))
                else:
                    # Guest image is at its preferred address: rebasing adds 0.
                    # Validate every record even though no pointer write is needed.
                    self.local_relocation_count += 1
        if self.has_dyld_info:
            return  # compressed dyld streams own the pointer sections
        for address, size, first in self.pointer_sections:
            count = size // 8
            if first > indirect_count or count > indirect_count - first:
                raise CompatibilityError("Indirect pointer section exceeds symbol table")
            for i in range(count):
                index, = unpack("<I", self.data, indirect_off + (first + i) * 4)
                target = address + i * 8
                self.segment_at(target, 8)
                if index & 0xc0000000:
                    if index not in (0x80000000, 0x40000000, 0xc0000000):
                        raise CompatibilityError("Invalid indirect local/absolute symbol marker")
                    continue  # already resolved locally; zero slide
                # Lazy slots contain linker stub addresses, not addends. Eagerly
                # replace both lazy and non-lazy slots with the resolved symbol.
                self.legacy_bindings.append((target, self.symbol_name(index), 0))

    @classmethod
    def from_path(cls, path):
        binary = resolve_binary(path)
        if binary.stat().st_size > MAX_IMAGE:
            raise CompatibilityError("Plugin file exceeds 256 MiB prototype limit")
        image = cls(binary.read_bytes())
        image.path = str(binary)
        return image

    def report(self):
        return {"architecture": "x86_64", "entry_address": self.entry,
                "dependencies": self.dependencies, "imports": sorted(set(self.imports)),
                "unsupported_loader_features": sorted(set(self.unsupported)),
                "legacy_local_relocations": self.local_relocation_count,
                "legacy_pointer_bindings": len(self.legacy_bindings),
                "segments": [{"name": s.name, "address": s.address, "bytes": s.size} for s in self.segments]}

    def bindings(self):
        """Decode classic dyld binding streams, including eager lazy binding."""
        yield from self.legacy_bindings
        yield from self.decode_bindings(self.bind_streams)

    def weak_bindings(self):
        yield from self.decode_bindings(self.weak_streams, weak=True)

    def decode_bindings(self, streams, weak=False):
        for data, lazy in streams:
            pos, segment, offset, symbol, addend, typ = 0, None, 0, None, 0, 1

            def leb(signed=False):
                nonlocal pos
                value, shift = 0, 0
                while True:
                    if pos >= len(data) or shift >= 64:
                        raise CompatibilityError("Invalid dyld LEB128 operand")
                    b = data[pos]
                    pos += 1
                    value |= (b & 127) << shift
                    shift += 7
                    if not b & 128:
                        value = value - (1 << shift) if signed and b & 64 else value
                        if not (-(1 << 63) <= value < (1 << 63) if signed else 0 <= value < (1 << 64)):
                            raise CompatibilityError("Dyld LEB128 operand exceeds 64 bits")
                        return value

            def binding():
                if segment is None or segment >= len(self.segments) or not symbol or typ != 1:
                    raise CompatibilityError("Unsupported or incomplete dyld pointer binding")
                s = self.segments[segment]
                if offset < 0 or offset + 8 > s.size:
                    raise CompatibilityError("Dyld binding outside segment")
                return s.address + offset, symbol, addend

            while pos < len(data):
                b = data[pos]
                pos += 1
                op, imm = b & 0xf0, b & 15
                if op == 0:
                    if not lazy:
                        break
                    segment, offset, symbol, addend, typ = None, 0, None, 0, 1
                elif op in (0x10, 0x30):
                    if weak:
                        raise CompatibilityError("Library ordinal in weak binding stream")
                    pass  # library ordinals; symbol shims are resolved explicitly
                elif op == 0x20:
                    if weak:
                        raise CompatibilityError("Library ordinal in weak binding stream")
                    leb()
                elif op == 0x40:
                    symbol = cstring(data, pos)
                    pos = data.index(0, pos) + 1
                    if weak and imm & 8:  # NON_WEAK_DEFINITION announcement
                        if symbol not in self.exports or symbol in self.weak_definitions:
                            raise CompatibilityError("Invalid strong-definition announcement")
                elif op == 0x50:
                    typ = imm
                elif op == 0x60:
                    addend = leb(True)
                elif op == 0x70:
                    segment, offset = imm, leb()
                elif op == 0x80:
                    # Linkers encode backward moves as unsigned 64-bit deltas.
                    # Match dyld's uint64_t arithmetic, then bounds-check binds.
                    offset = (offset + leb()) & ((1 << 64) - 1)
                elif op in (0x90, 0xa0, 0xb0):
                    yield binding()
                    offset = (offset + 8 + (leb() if op == 0xa0 else imm * 8 if op == 0xb0 else 0)) & ((1 << 64) - 1)
                elif op == 0xc0:
                    count, skip = leb(), leb()
                    if count > MAX_IMAGE // 8:
                        raise CompatibilityError("Too many dyld bindings")
                    for _ in range(count):
                        yield binding()
                        offset = (offset + 8 + skip) & ((1 << 64) - 1)
                else:
                    raise CompatibilityError(f"Unsupported dyld bind opcode 0x{op:02x}")
