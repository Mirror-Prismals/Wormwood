"""Owned offscreen Quartz contexts with i386 float/rectangle argument decoding."""
import ctypes
import math
import struct
import zlib
from macho import CompatibilityError
from libc_shims import string_bytes
from quickdraw32 import Rect

NO_ARGS = ('CGContextSaveGState', 'CGContextRestoreGState', 'CGContextBeginPath',
           'CGContextClosePath', 'CGContextSynchronize')
FLOAT_ARGS = {'CGContextTranslateCTM': 2, 'CGContextScaleCTM': 2, 'CGContextRotateCTM': 1,
              'CGContextSetAlpha': 1, 'CGContextSetLineWidth': 1,
              'CGContextSetRGBFillColor': 4, 'CGContextSetRGBStrokeColor': 4,
              'CGContextMoveToPoint': 2, 'CGContextAddLineToPoint': 2, 'CGContextSetTextPosition': 2}
RECT_ARGS = ('CGContextClipToRect', 'CGContextClearRect', 'CGContextFillRect', 'CGContextDrawImage')
INT_ARGS = ('CGContextSetShouldAntialias', 'CGContextDrawPath', 'CGContextSetTextDrawingMode')
COLOR_ARGS = ('CGContextSetFillColorSpace', 'CGContextSetStrokeColorSpace')
SYMBOLS = tuple('_' + name for name in (*NO_ARGS, *FLOAT_ARGS, *RECT_ARGS, *INT_ARGS, *COLOR_ARGS,
                                      'CGContextSelectFont', 'CGContextShowText', 'CGContextSetTextMatrix',
                                      'CGContextSetLineDash', 'CGContextGetTextPosition'))


class Point(ctypes.Structure):
    _fields_ = [('x', ctypes.c_double), ('y', ctypes.c_double)]


class Transform(ctypes.Structure):
    _fields_ = [(name, ctypes.c_double) for name in ('a', 'b', 'c', 'd', 'tx', 'ty')]


class Canvas32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.native = None
        self.surfaces = {}
        self.next_token = 0x76000000

    def prepare(self):
        if self.native is not None:
            return
        self.runtime.graphics.prepare()
        self.native = self.runtime.graphics.library
        signatures = {
            'CGBitmapContextCreate': [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint32],
            'CGContextRelease': [ctypes.c_void_p]}
        for name in NO_ARGS:
            signatures[name] = [ctypes.c_void_p]
        for name, count in FLOAT_ARGS.items():
            signatures[name] = [ctypes.c_void_p] + [ctypes.c_double] * count
        for name in RECT_ARGS:
            signatures[name] = [ctypes.c_void_p, Rect] + ([ctypes.c_void_p] if name == 'CGContextDrawImage' else [])
        for name in INT_ARGS:
            signatures[name] = [ctypes.c_void_p, ctypes.c_int]
        for name in COLOR_ARGS:
            signatures[name] = [ctypes.c_void_p, ctypes.c_void_p]
        signatures.update(CGContextSelectFont=[ctypes.c_void_p, ctypes.c_char_p, ctypes.c_double, ctypes.c_int],
                          CGContextShowText=[ctypes.c_void_p, ctypes.c_char_p, ctypes.c_size_t],
                          CGContextSetTextMatrix=[ctypes.c_void_p, Transform],
                          CGContextSetLineDash=[ctypes.c_void_p, ctypes.c_double, ctypes.c_void_p, ctypes.c_size_t])
        for name, args in signatures.items():
            function = getattr(self.native, name)
            function.argtypes = args
            function.restype = ctypes.c_void_p if name == 'CGBitmapContextCreate' else None
        self.native.CGContextGetTextPosition.argtypes = [ctypes.c_void_p]
        self.native.CGContextGetTextPosition.restype = Point

    def create(self, width, height):
        if (type(width) is not int or type(height) is not int or not 1 <= width <= 4096 or
                not 1 <= height <= 4096 or width * height > 4 * 1024 * 1024 or len(self.surfaces) >= 4):
            raise CompatibilityError('Invalid i386 editor surface dimensions')
        self.prepare()
        storage = ctypes.create_string_buffer(width * height * 4)
        color = self.native.CGColorSpaceCreateDeviceRGB()
        try:
            context = self.native.CGBitmapContextCreate(storage, width, height, 8, width * 4, color, 0x4001)
        finally:
            if color:
                self.native.CGColorSpaceRelease(color)
        if not context:
            raise CompatibilityError('Cannot allocate i386 editor bitmap')
        self.native.CGContextTranslateCTM(context, 0.0, float(height))
        self.native.CGContextScaleCTM(context, 1.0, -1.0)
        token = self.next_token
        self.next_token += 16
        self.surfaces[token] = dict(context=context, storage=storage, width=width, height=height, depth=0)
        return token

    def invoke(self, symbol, arg):
        w, name = self.runtime, symbol[1:]
        surface = self.surfaces.get(arg(0))
        if surface is None:
            raise CompatibilityError('Unknown i386 drawing context')
        context = surface['context']
        def floats(start, count):
            result = [struct.unpack('<f', struct.pack('<I', arg(i)))[0] for i in range(start, start + count)]
            if any(not math.isfinite(v) for v in result):
                raise CompatibilityError('Non-finite i386 drawing argument')
            return result
        function = getattr(self.native, name)
        if name == 'CGContextGetTextPosition':
            point = function(context)
            x, y = struct.unpack('<II', struct.pack('<ff', point.x, point.y))
            w.cpu.reg_write(w.x86.UC_X86_REG_EDX, y)
            return x  # Darwin i386 CGPoint (8 bytes) returns in EDX:EAX.
        if name in NO_ARGS:
            if name == 'CGContextSaveGState':
                if surface['depth'] >= 128:
                    raise CompatibilityError('i386 graphics-state stack exceeded')
                surface['depth'] += 1
            elif name == 'CGContextRestoreGState':
                if not surface['depth']:
                    raise CompatibilityError('i386 graphics-state stack underflow')
                surface['depth'] -= 1
            function(context)
        elif name in FLOAT_ARGS:
            function(context, *floats(1, FLOAT_ARGS[name]))
        elif name in RECT_ARGS:
            rect = Rect(*floats(1, 4))
            if name == 'CGContextDrawImage':
                image = w.graphics.images.get(arg(5))
                if image is None:
                    raise CompatibilityError('Unknown i386 drawn image')
                function(context, rect, image['pointer'])
            else:
                function(context, rect)
        elif name in INT_ARGS:
            function(context, arg(1))
        elif name in COLOR_ARGS:
            color = w.graphics.objects.get(arg(1))
            if color is None:
                raise CompatibilityError('Unknown i386 drawing color space')
            function(context, color)
        elif name == 'CGContextSelectFont':
            function(context, string_bytes(w, arg(1), limit=1024), *floats(2, 1), arg(3))
        elif name == 'CGContextShowText':
            if arg(2) > 65536:
                raise CompatibilityError('Oversized i386 drawing text')
            function(context, w.read(arg(1), arg(2)), arg(2))
        elif name == 'CGContextSetTextMatrix':
            function(context, Transform(*floats(1, 6)))
        elif name == 'CGContextSetLineDash':
            count = arg(3)
            if count > 256:
                raise CompatibilityError('Oversized i386 line dash')
            values = struct.unpack('<' + 'f' * count, w.read(arg(2), count * 4)) if count else ()
            if any(not math.isfinite(v) or v < 0 for v in values):
                raise CompatibilityError('Invalid i386 line dash')
            function(context, *floats(1, 1), (ctypes.c_double * count)(*values), count)
        return 0

    def png(self, token):
        surface = self.surfaces[token]
        width, height = surface['width'], surface['height']
        rgba = bytearray(surface['storage'].raw)
        # Core Graphics stores premultiplied RGBA; PNG stores straight alpha.
        for offset in range(0, len(rgba), 4):
            alpha = rgba[offset + 3]
            if 0 < alpha < 255:
                for channel in range(3):
                    rgba[offset + channel] = min(255, (rgba[offset + channel] * 255 + alpha // 2) // alpha)
        raw = b''.join(b'\0' + rgba[row * width * 4:(row + 1) * width * 4] for row in range(height))
        def chunk(kind, payload):
            return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind + payload))
        return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')

    def close(self):
        if self.native:
            for surface in self.surfaces.values():
                self.native.CGContextRelease(surface['context'])
            self.surfaces.clear()
