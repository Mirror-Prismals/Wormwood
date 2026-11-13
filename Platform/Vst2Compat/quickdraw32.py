"""Bounded offscreen 32-bit QuickDraw worlds; no native Carbon windows."""
import ctypes
import struct
from macho import CompatibilityError
from libc_shims import release

SYMBOLS = ('_NewGWorld', '_DisposeGWorld', '_GetGWorldPixMap', '_GetPixBaseAddr',
           '_GetPixBounds', '_GetPixDepth', '_GetPixRowBytes', '_LockPixels', '_UnlockPixels')


class Rect(ctypes.Structure):
    _fields_ = [(name, ctypes.c_double) for name in ('x', 'y', 'width', 'height')]


class QuickDraw32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.worlds = {}
        self.pixmaps = {}
        self.next_world = 0x75f00000
        self.native = None

    def invoke(self, name, arg):
        w = self.runtime
        if name == '_NewGWorld':
            output, depth, bounds, table, device, flags = (arg(i) for i in range(6))
            if depth not in (0, 32) or table or device or flags:
                raise CompatibilityError('Unsupported i386 offscreen pixel format/options')
            top, left, bottom, right = struct.unpack('<4h', w.read(bounds, 8))
            width, height = right - left, bottom - top
            if not (1 <= width <= 4095 and 1 <= height <= 4096) or width * height > 4 * 1024 * 1024:
                return -50
            size = width * height * 4
            if len(self.worlds) >= 256 or self.next_world >= 0x76000000 or sum(r['size'] for r in self.worlds.values()) + size > 16 * 1024 * 1024:
                raise CompatibilityError('i386 offscreen pixel budget exceeded')
            pixels, pixmap, handle = w.allocate(size), w.allocate(50), w.allocate(4)
            w.cpu.mem_write(pixels, bytes(size))
            w.cpu.mem_write(pixmap, struct.pack('<IH4hHHIIIHHHHIII', pixels, width * 4 | 0x8000,
                top, left, bottom, right, 0, 0, 0, 72 << 16, 72 << 16, 16, 32, 3, 8, 0, 0, 0))
            w.cpu.mem_write(handle, struct.pack('<I', pixmap))
            token = self.next_world
            self.next_world += 16
            record = dict(pixels=pixels, pixmap=pixmap, handle=handle, bounds=(top, left, bottom, right),
                          width=width, height=height, size=size, locked=False)
            self.worlds[token] = record
            self.pixmaps[handle] = record
            w.cpu.mem_write(output, struct.pack('<I', token))
            return 0
        if name in ('_DisposeGWorld', '_GetGWorldPixMap'):
            record = self.worlds.get(arg(0))
            if record is None:
                raise CompatibilityError('Unknown i386 offscreen world')
            if name == '_GetGWorldPixMap':
                return record['handle']
            del self.worlds[arg(0)]
            del self.pixmaps[record['handle']]
            for field in ('pixels', 'pixmap', 'handle'):
                release(w, record[field])
            return 0
        record = self.pixmaps.get(arg(0))
        if record is None:
            raise CompatibilityError('Unknown i386 pixel-map handle')
        if name == '_GetPixBaseAddr':
            return record['pixels']
        if name == '_GetPixDepth':
            return 32
        if name == '_GetPixRowBytes':
            return record['width'] * 4
        if name == '_GetPixBounds':
            w.cpu.mem_write(arg(1), struct.pack('<4h', *record['bounds']))
            return arg(1)
        record['locked'] = name == '_LockPixels'
        return 1 if record['locked'] else 0

    def draw_image(self, world, image):
        w = self.runtime
        if world not in self.worlds or image not in w.graphics.images:
            raise CompatibilityError('Unknown i386 image drawing target')
        record = self.worlds[world]
        if self.native is None:
            w.graphics.prepare()
            self.native = w.graphics.library
            signatures = {
                'CGBitmapContextCreate': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint32]),
                'CGContextTranslateCTM': (None, [ctypes.c_void_p, ctypes.c_double, ctypes.c_double]),
                'CGContextScaleCTM': (None, [ctypes.c_void_p, ctypes.c_double, ctypes.c_double]),
                'CGContextDrawImage': (None, [ctypes.c_void_p, Rect, ctypes.c_void_p]),
                'CGContextRelease': (None, [ctypes.c_void_p])}
            for name, (result, arguments) in signatures.items():
                function = getattr(self.native, name)
                function.argtypes, function.restype = arguments, result
        storage = ctypes.create_string_buffer(w.read(record['pixels'], record['size']), record['size'])
        color = self.native.CGColorSpaceCreateDeviceRGB()
        context = None
        try:
            if not color:
                raise CompatibilityError('Cannot allocate offscreen color space')
            context = self.native.CGBitmapContextCreate(storage, record['width'], record['height'], 8,
                                                       record['width'] * 4, color, 0x4006)  # big-endian XRGB
            if not context:
                raise CompatibilityError('Cannot allocate offscreen bitmap context')
            # Bitmap-context storage already yields top-to-bottom image rows.
            # Flipping its CTM here would invert QuickDraw's pixel memory.
            self.native.CGContextDrawImage(context, Rect(0, 0, record['width'], record['height']), w.graphics.images[image]['pointer'])
            w.cpu.mem_write(record['pixels'], storage.raw)
        finally:
            if context:
                self.native.CGContextRelease(context)
            if color:
                self.native.CGColorSpaceRelease(color)
