"""Minimal i386 CF bundle discovery; native executable pointers never escape."""
import ctypes
import struct
from pathlib import Path
import format_shims
import libc_shims
from format32 import Arguments32

from macho import CompatibilityError

SYMBOLS = ('_CFBundleGetBundleWithIdentifier', '_CFBundleGetFunctionPointerForName',
           '_CFStringCreateWithFormat', '_CFStringCreateWithCString', '_CFStringGetCString',
           '_CFStringGetLength', '_CFRetain', '_CFRelease',
           '_CFURLCreateFromFileSystemRepresentation', '_CFURLCreateCopyDeletingLastPathComponent',
           '_CFBundleCreate', '_CFBundleCopyResourceURL', '_CFURLGetFileSystemRepresentation', '_CFURLGetFSRef',
           '_CFURLCopyPathExtension', '_CFStringCompare')


class CF32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.library = None
        self.bundles = {}
        self.identifiers = {}
        self.lookups = []
        self.strings = {}
        self.urls = {}
        self.guest_bundles = {}
        self.next_string = 0x75210000

    def wrap_string(self, value):
        if len(value) > 65536:
            raise CompatibilityError('i386 CF string limit exceeded')
        return self.wrap(self.strings, value)

    def wrap(self, table, value):
        if sum(map(len, (self.strings, self.urls, self.guest_bundles))) >= 1024 or self.next_string >= 0x75300000:
            raise CompatibilityError('i386 CF object limit exceeded')
        token = self.next_string
        self.next_string += 16
        table[token] = [value, 1]
        return token

    def get(self, table, token):
        if token not in table:
            raise CompatibilityError('Unknown i386 CF object')
        return table[token][0]

    @staticmethod
    def encoding(value):
        encodings = {0: 'mac_roman', 0x08000100: 'utf-8', 0x0600: 'ascii', 0x0201: 'latin-1'}
        if value not in encodings:
            raise CompatibilityError('Unsupported i386 CF string encoding')
        return encodings[value]

    def constant_string(self, address):
        if address in self.strings:
            return self.strings[address][0]
        w = self.runtime
        isa, flags, data, length = struct.unpack('<4I', w.read(address, 16))
        if isa != w.missing.get('___CFConstantStringClassReference') or flags not in (0x7c8, 0x7d0) or length > 65536:
            raise CompatibilityError('Unsupported i386 CF string representation')
        return w.read(data, length * (1 if flags == 0x7c8 else 2)).decode('ascii' if flags == 0x7c8 else 'utf-16-le')

    def prepare(self):
        if self.library is not None:
            return
        self.library = ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
        signatures = {
            'CFStringCreateWithCString': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint32]),
            'CFBundleGetBundleWithIdentifier': (ctypes.c_void_p, [ctypes.c_void_p]),
            'CFRetain': (ctypes.c_void_p, [ctypes.c_void_p]),
            'CFRelease': (None, [ctypes.c_void_p]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.library, name)
            function.restype, function.argtypes = result, arguments

    def invoke(self, name, arg):
        w = self.runtime
        if name == '_CFURLCopyPathExtension':
            extension = self.get(self.urls, arg(0)).suffix
            return self.wrap_string(extension[1:]) if extension else 0
        if name == '_CFStringCompare':
            if arg(2):
                raise CompatibilityError('i386 CF string comparison options unsupported')
            a, b = self.constant_string(arg(0)), self.constant_string(arg(1))
            return (a > b) - (a < b)
        if name in ('_CFRetain', '_CFRelease'):
            token = arg(0)
            table = next((table for table in (self.strings, self.urls, self.guest_bundles) if token in table), None)
            if table is not None:
                record = table[token]
                record[1] += 1 if name == '_CFRetain' else -1
                if record[1] == 0:
                    del table[token]
            else:
                # Only immutable guest constant strings have no ownership cost.
                self.constant_string(token)
            return token if name == '_CFRetain' else 0
        if name in ('_CFURLCreateFromFileSystemRepresentation', '_CFURLCreateCopyDeletingLastPathComponent', '_CFBundleCreate'):
            if arg(0):
                raise CompatibilityError('Custom i386 CF allocator unsupported')
            if name == '_CFURLCreateFromFileSystemRepresentation':
                if arg(2) > 65536:
                    raise CompatibilityError('Oversized i386 file URL')
                value = w.read(arg(1), arg(2)).decode('utf-8')
                if '\0' in value or not Path(value).is_absolute():
                    raise CompatibilityError('Invalid absolute i386 file URL')
                return self.wrap(self.urls, Path(value))
            path = self.get(self.urls, arg(1))
            if name == '_CFURLCreateCopyDeletingLastPathComponent':
                return self.wrap(self.urls, path.parent)
            if not path.is_dir() or not (path / 'Contents/Info.plist').is_file():
                return 0
            # Resource-only bundle; never loads its executable in the host.
            return self.wrap(self.guest_bundles, path.resolve())
        if name == '_CFBundleCopyResourceURL':
            bundle = self.get(self.guest_bundles, arg(0))
            base = (bundle / 'Contents/Resources').resolve()
            filename = self.constant_string(arg(1))
            extension = self.constant_string(arg(2)) if arg(2) else ''
            subdirectory = self.constant_string(arg(3)) if arg(3) else ''
            if any('\0' in part for part in (filename, extension, subdirectory)):
                return 0
            path = (base / subdirectory / (filename + ('.' + extension if extension else ''))).resolve()
            if not path.is_relative_to(base) or not path.is_file():
                return 0
            return self.wrap(self.urls, path)
        if name == '_CFURLGetFileSystemRepresentation':
            value = str(self.get(self.urls, arg(0))).encode('utf-8') + b'\0'
            if arg(3) > 65536:
                raise CompatibilityError('Oversized i386 URL output')
            if len(value) > arg(3):
                return 0
            w.cpu.mem_write(arg(2), value)
            return 1
        if name == '_CFURLGetFSRef':
            path = str(self.get(self.urls, arg(0))).encode('utf-8')
            w.filesystem.load_native()
            native = ctypes.create_string_buffer(80)
            result = w.filesystem.native.FSPathMakeRef(w.resource_paths.resolve(path), native, None)
            if result != 0:
                return 0
            w.filesystem.save_reference(arg(1), native)
            return 1
        if name == '_CFStringCreateWithFormat':
            if arg(0) or arg(1):
                raise CompatibilityError('Custom i386 CF allocator/format options unsupported')
            value = self.constant_string(arg(2))
            if '\0' in value:
                raise CompatibilityError('Embedded NUL in i386 CF format')
            data = value.encode('utf-8') + b'\0'
            pointer = w.allocate(len(data))
            try:
                w.cpu.mem_write(pointer, data)
                result = format_shims.format_bytes(w, pointer, Arguments32(w, 3))
                return self.wrap_string(result.decode('utf-8'))
            finally:
                libc_shims.release(w, pointer)
        if name == '_CFStringCreateWithCString':
            if arg(0):
                raise CompatibilityError('Custom i386 CF allocator unsupported')
            try:
                value = libc_shims.string_bytes(w, arg(1), limit=65536).decode(self.encoding(arg(2)))
            except UnicodeError:
                return 0
            return self.wrap_string(value)
        if name == '_CFStringGetLength':
            return len(self.constant_string(arg(0)).encode('utf-16-le')) // 2
        if name == '_CFStringGetCString':
            size = arg(2)
            if size > 65536:
                raise CompatibilityError('Oversized i386 CF string output')
            try:
                data = self.constant_string(arg(0)).encode(self.encoding(arg(3))) + b'\0'
            except UnicodeError:
                return 0
            if len(data) > size:
                return 0
            w.cpu.mem_write(arg(1), data)
            return 1
        self.prepare()
        if name == '_CFBundleGetBundleWithIdentifier':
            identifier = self.constant_string(arg(0))
            if identifier in self.identifiers:
                return self.identifiers[identifier]
            if len(self.bundles) >= 128:
                raise CompatibilityError('i386 bundle lookup limit exceeded')
            string = self.library.CFStringCreateWithCString(None, identifier.encode('utf-8'), 0x08000100)
            if not string:
                return 0
            try:
                bundle = self.library.CFBundleGetBundleWithIdentifier(string)
                if not bundle:
                    return 0
                self.library.CFRetain(bundle)
            finally:
                self.library.CFRelease(string)
            token = 0x75200000 + len(self.bundles) * 16
            self.bundles[token] = bundle
            self.identifiers[identifier] = token
            return token
        if arg(0) not in self.bundles:
            raise CompatibilityError('Unknown i386 bundle token')
        function = self.constant_string(arg(1))
        if len(self.lookups) < 128:
            self.lookups.append(function)
        # Expose only implemented guest ABI gates. An optional function not
        # provided by this compatibility environment is unavailable (NULL).
        return self.runtime.shims.get('_' + function, 0)

    def close(self):
        self.strings.clear()
        self.urls.clear()
        self.guest_bundles.clear()
        if self.library is not None:
            for pointer in self.bundles.values():
                self.library.CFRelease(pointer)
            self.bundles.clear()
            self.identifiers.clear()
