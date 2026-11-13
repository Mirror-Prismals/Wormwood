"""Read-only i386 Mach-O loader metadata, separate from the x86_64 host.

Legacy images remain at their preferred address. Pointer imports and five-byte
Intel jump stubs are reported separately; a runtime must never write pointers
over executable stubs. No native dyld or plugin entry point is invoked here.
"""
import struct

from macho import CompatibilityError, MAX_IMAGE, MachO, Segment, checked, cstring, unpack

I386 = 7


def i386_slice(data):
    formats = {b"\xca\xfe\xba\xbe": (">", False), b"\xbe\xba\xfe\xca": ("<", False),
               b"\xca\xfe\xba\xbf": (">", True), b"\xbf\xba\xfe\xca": ("<", True)}
    magic = checked(data, 0, 4)
    if magic not in formats:
        return data
    endian, wide = formats[magic]
    count, = unpack(endian + "I", data, 4)
    if count > 64:
        raise CompatibilityError("Too many universal binary slices")
    fmt = endian + ("IIQQII" if wide else "IIIII")
    stride = struct.calcsize(fmt)
    checked(data, 8, count * stride)
    for index in range(count):
        cpu, subtype, offset, size, *_ = unpack(fmt, data, 8 + index * stride)
        if cpu == I386:
            return checked(data, offset, size)
    raise CompatibilityError("No Intel i386 slice; PowerPC emulation is not supported")


class MachO32(MachO):
    architecture = "i386"
    pointer_size = 4

    def __init__(self, data):
        self.data = i386_slice(data)
        magic, cpu, subtype, kind, count, size, flags = unpack("<7I", self.data)
        if magic != 0xfeedface or cpu != I386:
            raise CompatibilityError("Expected a little-endian Intel i386 Mach-O plugin")
        if kind not in (6, 8):
            raise CompatibilityError("Expected MH_BUNDLE or MH_DYLIB")
        if count > 4096:
            raise CompatibilityError("Too many Mach-O load commands")
        checked(self.data, 28, size)
        self.kind = kind
        self.segments, self.dependencies, self.initializers = [], [], []
        self.symbols, self.imports, self.unsupported = {}, [], []
        self.exports, self.weak_definitions = {}, set()
        self.symbol_table, self.pointer_sections, self.stub_sections = [], [], []
        self.legacy_bindings, self.stub_bindings = [], []
        self.local_relocation_count = 0
        self.dysymtab = None
        if flags & 0x10:
            self.unsupported.append("prebound image")
        symtab = None
        pos, end = 28, 28 + size
        for _ in range(count):
            cmd, length = unpack("<II", self.data, pos)
            if length < 8 or length % 4 or pos + length > end:
                raise CompatibilityError("Invalid Mach-O load command size")
            blob = checked(self.data, pos, length)
            if cmd == 1:  # LC_SEGMENT, section (not section_64)
                _, _, name, addr, memsize, off, nbytes, maxprot, prot, nsects, segflags = unpack("<II16sIIIIiiII", blob)
                if memsize > MAX_IMAGE or nbytes > memsize or addr + memsize > 1 << 32:
                    raise CompatibilityError("Segment exceeds i386 guest image limits")
                checked(self.data, off, nbytes)
                checked(blob, 56, nsects * 68)
                self.segments.append(Segment(name.rstrip(b"\0").decode("ascii", errors="replace"),
                                             addr, memsize, off, nbytes, prot & 7))
                for index in range(nsects):
                    section = unpack("<16s16s9I", blob, 56 + index * 68)
                    secaddr, secsize, secflags = section[2], section[3], section[8]
                    if secaddr < addr or secaddr + secsize > addr + memsize:
                        raise CompatibilityError("Section outside its segment")
                    if section[7]:
                        self.unsupported.append("section relocations")
                    typ = secflags & 255
                    if typ in (6, 7, 9):
                        if secsize % 4:
                            raise CompatibilityError("Invalid i386 pointer section size")
                        if typ == 9:
                            self.initializers.extend(range(secaddr, secaddr + secsize, 4))
                        else:
                            self.pointer_sections.append((secaddr, secsize, section[9]))
                    elif typ == 8:
                        stride = section[10]
                        if stride != 5 or secsize % stride:
                            self.unsupported.append("non-five-byte i386 symbol stubs")
                        else:
                            self.stub_sections.append((secaddr, secsize, section[9]))
                    elif 0x11 <= typ <= 0x15:
                        self.unsupported.append("thread-local storage")
            elif cmd == 2:
                if symtab is not None:
                    raise CompatibilityError("Duplicate symbol table")
                _, _, *symtab = unpack("<6I", blob)
            elif cmd == 11:
                if self.dysymtab is not None:
                    raise CompatibilityError("Duplicate dynamic symbol table")
                self.dysymtab = unpack("<20I", blob)
            elif cmd in (0xc, 0x80000018, 0x8000001f, 0x80000023):
                nameoff, = unpack("<I", blob, 8)
                if nameoff < 24:
                    raise CompatibilityError("Invalid dependency name offset")
                self.dependencies.append(cstring(blob, nameoff))
            elif cmd in (0x22, 0x80000022, 0x80000034, 0x80000033):
                self.unsupported.append("compressed i386 dyld metadata")
            pos += length
        if pos != end:
            raise CompatibilityError("Load command count does not match command bytes")
        ranges = sorted((s.address, s.address + s.size) for s in self.segments if s.size)
        if sum(s.size for s in self.segments) > MAX_IMAGE:
            raise CompatibilityError("Image exceeds guest memory limit")
        if any(a[1] > b[0] for a, b in zip(ranges, ranges[1:])):
            raise CompatibilityError("Overlapping Mach-O segments")
        if symtab is not None:
            off, count, stroff, strsize = symtab
            checked(self.data, off, count * 12)
            strings = checked(self.data, stroff, strsize)
            for index in range(count):
                nameoff, typ, sect, desc, value = unpack("<IBBHI", self.data, off + index * 12)
                name = cstring(strings, nameoff) if nameoff else ""
                self.symbol_table.append((name, typ, value))
                if typ & 0xe0 or not nameoff:
                    continue
                if typ & 0xe == 0:
                    self.imports.append(name)
                elif (typ & 0xe) in (2, 0xe):
                    self.symbols[name] = value
                    if typ & 1 and not typ & 0x10:
                        self.exports[name] = value
                        if desc & 0x80:
                            self.weak_definitions.add(name)
        if self.dysymtab is not None:
            self.parse_i386_tables(flags)
        elif self.pointer_sections or self.stub_sections:
            raise CompatibilityError("Indirect sections require a dynamic symbol table")
        self.entry = next((self.symbols[n] for n in ("_VSTPluginMain", "_main_macho", "_main")
                           if n in self.symbols), None)

    def pointer_at(self, address):
        segment = self.segment_at(address, 4)
        offset = address - segment.address
        if offset >= segment.file_size:
            return 0
        return int.from_bytes(checked(self.data, segment.offset + offset,
                                      min(4, segment.file_size - offset)).ljust(4, b"\0"), "little")

    def parse_i386_tables(self, flags):
        indirect_off, indirect_count, ext_off, ext_count, loc_off, loc_count = self.dysymtab[14:20]
        if indirect_count + ext_count + loc_count > 1_000_000:
            raise CompatibilityError("Too many legacy dynamic relocation entries")
        checked(self.data, indirect_off, indirect_count * 4)
        checked(self.data, ext_off, ext_count * 8)
        checked(self.data, loc_off, loc_count * 8)
        # Unlike x86_64, non-split i386 relocations use the first segment.
        base = next((s.address for s in self.segments if not flags & 0x20 or s.protection & 2), None)
        if (ext_count or loc_count) and base is None:
            raise CompatibilityError("Legacy relocations have no base segment")
        for off, count, external in ((loc_off, loc_count, False), (ext_off, ext_count, True)):
            for index in range(count):
                relative, bits = unpack("<II", self.data, off + index * 8)
                if relative & 0x80000000:
                    self.unsupported.append("scattered i386 relocations")
                    continue
                symbol = bits & 0xffffff
                pcrel, width, is_external, kind = (bits >> 24) & 1, (bits >> 25) & 3, (bits >> 27) & 1, bits >> 28
                if pcrel or width != 2 or is_external != external or kind != 0:
                    raise CompatibilityError("Unsupported i386 relocation (requires vanilla 32-bit pointer)")
                address = base + relative
                self.segment_at(address, 4)
                if external:
                    self.legacy_bindings.append((address, self.symbol_name(symbol), self.pointer_at(address)))
                else:
                    self.local_relocation_count += 1  # preferred address, zero slide
        for sections, stride, output in ((self.pointer_sections, 4, self.legacy_bindings),
                                          (self.stub_sections, 5, self.stub_bindings)):
            for address, size, first in sections:
                count = size // stride
                if first > indirect_count or count > indirect_count - first:
                    raise CompatibilityError("Indirect section exceeds symbol table")
                for index in range(count):
                    symbol, = unpack("<I", self.data, indirect_off + (first + index) * 4)
                    target = address + index * stride
                    self.segment_at(target, stride)
                    if symbol & 0xc0000000:
                        if symbol not in (0x80000000, 0x40000000, 0xc0000000):
                            raise CompatibilityError("Invalid indirect local/absolute symbol marker")
                        continue
                    output.append((target, self.symbol_name(symbol), 0))

    def bindings(self):
        yield from self.legacy_bindings

    def weak_bindings(self):
        return iter(())

    def report(self):
        report = super().report()
        report.update(architecture=self.architecture, pointer_bytes=4,
                      legacy_jump_stubs=len(self.stub_bindings),
                      initializer_count=len(self.initializers))
        return report
