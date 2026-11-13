"""Opt-in bitmap focus for disposable editor probes; no visible windows."""
import ctypes
import math
import struct
from macho import CompatibilityError
from libc_shims import release
from view_shims import Point, Size, Rect


class Transform(ctypes.Structure):
    _fields_ = [(name, ctypes.c_double) for name in ('a', 'b', 'c', 'd', 'tx', 'ty')]

NO_ARGS = ('CGContextSaveGState', 'CGContextRestoreGState', 'CGContextBeginPath',
           'CGContextClosePath', 'CGContextSynchronize')
FLOAT_ARGS = {'CGContextTranslateCTM': 2, 'CGContextScaleCTM': 2, 'CGContextRotateCTM': 1,
              'CGContextSetAlpha': 1, 'CGContextSetLineWidth': 1,
              'CGContextSetRGBFillColor': 4, 'CGContextSetRGBStrokeColor': 4,
              'CGContextMoveToPoint': 2, 'CGContextAddLineToPoint': 2}
RECT_ARGS = ('CGContextClipToRect', 'CGContextClearRect', 'CGContextFillRect', 'CGContextDrawImage')
INT_ARGS = {'CGContextSetShouldAntialias': (ctypes.c_bool, 1), 'CGContextDrawPath': (ctypes.c_int, 4)}
COLOR_ARGS = ('CGContextSetFillColorSpace', 'CGContextSetStrokeColorSpace')
COLOR_APIS = ('CGColorSpaceCreateWithName', 'CGColorSpaceCreateDeviceRGB', 'CGColorSpaceRelease')
COMPLEX_APIS = ('CGContextSetTextMatrix', 'CGContextGetCTM', 'CGContextSetLineDash',
                'CGContextStrokeLineSegments', 'CGContextAddArc')
SYMBOLS = tuple('_' + n for n in (*NO_ARGS, *FLOAT_ARGS, *RECT_ARGS, *INT_ARGS, *COLOR_ARGS, *COLOR_APIS, *COMPLEX_APIS))


class Graphics:
    def __init__(self, registry):
        self.registry = registry
        self.surfaces = {}
        self.stack = []
        self.native = ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
        signatures = {
            'CGColorSpaceCreateDeviceRGB': (ctypes.c_void_p, []),
            'CGColorSpaceCreateWithName': (ctypes.c_void_p, [ctypes.c_void_p]),
            'CGColorSpaceRelease': (None, [ctypes.c_void_p]),
            'CGBitmapContextCreate': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t,
                ctypes.c_size_t, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint32]),
            'CGContextRelease': (None, [ctypes.c_void_p]),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.native, name)
            function.restype, function.argtypes = result, arguments
        for name in NO_ARGS:
            self.configure(name, [])
        for name, count in FLOAT_ARGS.items():
            self.configure(name, [ctypes.c_double] * count)
        for name in RECT_ARGS:
            self.configure(name, [Rect] + ([ctypes.c_void_p] if name == 'CGContextDrawImage' else []))
        for name, (kind, _) in INT_ARGS.items():
            self.configure(name, [kind])
        for name in COLOR_ARGS:
            self.configure(name, [ctypes.c_void_p])
        self.configure('CGContextSetTextMatrix', [Transform])
        self.configure('CGContextSetLineDash', [ctypes.c_double, ctypes.POINTER(ctypes.c_double), ctypes.c_size_t])
        self.configure('CGContextStrokeLineSegments', [ctypes.POINTER(Point), ctypes.c_size_t])
        self.configure('CGContextAddArc', [ctypes.c_double] * 5 + [ctypes.c_int])
        self.native.CGContextGetCTM.restype = Transform
        self.native.CGContextGetCTM.argtypes = [ctypes.c_void_p]

    def doubles(self, count):
        w = self.registry.worker
        values = [struct.unpack('<d', struct.pack('<Q', w.cpu.reg_read(
            getattr(w.x86, f'UC_X86_REG_XMM{i}')) & ((1 << 64) - 1)))[0] for i in range(count)]
        self.validate_numbers(values)
        return values

    def validate_numbers(self, values):
        if not all(math.isfinite(v) and abs(v) <= 1e6 for v in values):
            raise CompatibilityError('Unsupported graphics scalar')

    def configure(self, name, arguments):
        function = getattr(self.native, name)
        function.restype, function.argtypes = None, [ctypes.c_void_p, *arguments]

    def invoke(self, symbol):
        w, name = self.registry.worker, symbol[1:]
        if name == 'CGColorSpaceCreateWithName':
            with w.cf.string(w.arg(0)) as color_name:
                return w.cf.wrap(self.native.CGColorSpaceCreateWithName(color_name), 'color_space')
        if name == 'CGColorSpaceCreateDeviceRGB':
            return w.cf.wrap(self.native.CGColorSpaceCreateDeviceRGB(), 'color_space')
        if name == 'CGColorSpaceRelease':
            w.cf.get(w.arg(0), 'color_space')
            return w.cf.invoke('_CFRelease')
        port = w.arg(1) if name == 'CGContextGetCTM' else w.arg(0)
        surface = next((s for s in self.surfaces.values() if s['port'] == port), None)
        if surface is None:
            raise CompatibilityError('Drawing requires a known bitmap context handle')
        arguments = []
        if name == 'CGContextGetCTM':
            transform = self.native.CGContextGetCTM(surface['context'])
            values = [getattr(transform, field) for field, _ in Transform._fields_]
            w.cpu.mem_write(w.arg(0), struct.pack('<6d', *values))
            return w.arg(0)
        if name == 'CGContextSetTextMatrix':
            values = struct.unpack('<6d', w.read(w.cpu.reg_read(w.x86.UC_X86_REG_RSP) + 8, 48))
            self.validate_numbers(values)
            arguments = [Transform(*values)]
        elif name == 'CGContextSetLineDash':
            count = w.arg(2)
            if count > 1024:
                raise CompatibilityError('Line dash count exceeds limit')
            values = struct.unpack('<' + 'd' * count, w.read(w.arg(1), count * 8)) if count else ()
            self.validate_numbers(values)
            if values and (any(v < 0 for v in values) or not any(values)):
                raise CompatibilityError('Invalid line dash lengths')
            arguments = [self.doubles(1)[0], (ctypes.c_double * count)(*values), count]
        elif name == 'CGContextStrokeLineSegments':
            count = w.arg(2)
            if count > 8192 or count % 2:
                raise CompatibilityError('Invalid line segment point count')
            values = struct.unpack('<' + 'd' * (count * 2), w.read(w.arg(1), count * 16)) if count else ()
            self.validate_numbers(values)
            arguments = [(Point * count)(*(Point(*values[i:i+2]) for i in range(0, count * 2, 2))), count]
        elif name == 'CGContextAddArc':
            arguments = self.doubles(5)
            if arguments[2] < 0 or w.arg(1) > 1:
                raise CompatibilityError('Invalid arc radius or direction')
            arguments.append(w.arg(1))
        elif name in FLOAT_ARGS:
            arguments = self.doubles(FLOAT_ARGS[name])
        elif name in RECT_ARGS:
            values = struct.unpack('<4d', w.read(w.cpu.reg_read(w.x86.UC_X86_REG_RSP) + 8, 32))
            if not all(math.isfinite(v) and abs(v) <= 1e6 for v in values) or any(v < 0 for v in values[2:]):
                raise CompatibilityError('Unsupported graphics rectangle')
            arguments = [Rect(Point(*values[:2]), Size(*values[2:]))]
            if name == 'CGContextDrawImage':
                arguments.append(w.cf.get(w.arg(1), 'image'))
        elif name in INT_ARGS:
            value = w.arg(1)
            if value > INT_ARGS[name][1]:
                raise CompatibilityError('Unsupported graphics enum value')
            arguments = [value]
        elif name in COLOR_ARGS:
            arguments = [w.cf.get(w.arg(1), 'color_space')]
        elif name not in NO_ARGS:
            raise CompatibilityError(f'Unsupported bitmap operation {name}')
        if name == 'CGContextSaveGState':
            if surface['saved_states'] >= 128:
                raise CompatibilityError('Bitmap graphics state nesting exceeds limit')
            surface['saved_states'] += 1
        elif name == 'CGContextRestoreGState':
            if not surface['saved_states']:
                raise CompatibilityError('Unbalanced bitmap graphics state restore')
            surface['saved_states'] -= 1
        getattr(self.native, name)(surface['context'], *arguments)
        return 0

    def surface(self, view):
        if view in self.surfaces:
            return self.surfaces[view]
        r = self.registry
        frame = r.views.records[view]['frame']
        width, height = (math.ceil(v) for v in frame[2:])
        if not (1 <= width <= 4096 and 1 <= height <= 4096) or width * height > 4 * 1024 * 1024:
            raise CompatibilityError('Editor bitmap exceeds diagnostic surface limit')
        if width * height + sum(s['width'] * s['height'] for s in self.surfaces.values()) > 16 * 1024 * 1024:
            raise CompatibilityError('Editor bitmaps exceed total diagnostic memory limit')
        pixels = ctypes.create_string_buffer(width * height * 4)
        color = self.native.CGColorSpaceCreateDeviceRGB()
        if not color:
            raise CompatibilityError('Cannot allocate editor color space')
        try:
            # Explicit RGBA, 8-bit channels, premultiplied alpha last.
            context = self.native.CGBitmapContextCreate(pixels, width, height, 8, width * 4, color, 0x4001)
        finally:
            self.native.CGColorSpaceRelease(color)
        if not context:
            raise CompatibilityError('Cannot allocate editor bitmap')
        flipped = bool(r.views.records[view].get('flipped', False))
        if flipped:
            self.native.CGContextTranslateCTM(context, 0., float(height))
            self.native.CGContextScaleCTM(context, 1., -1.)
        native_context = None
        allocated = []
        try:
            cls = r.builtin('NSGraphicsContext')
            native_context = r.bridge.message_args(r.bridge.native.objc_getClass(b'NSGraphicsContext'),
                'graphicsContextWithCGContext:flipped:', [ctypes.c_void_p, ctypes.c_bool], context, flipped)
            if not native_context:
                raise CompatibilityError('Cannot create bitmap NSGraphicsContext')
            r.bridge.message(native_context, 'retain')
            pointer = r.allocate(16)
            allocated.append(pointer)
            port = r.allocate(16)
            allocated.append(port)
            r.worker.write_u64(pointer, cls)
            r.instances[pointer] = [cls, 1]
            result = dict(pointer=pointer, port=port, native=native_context, context=context,
                          pixels=pixels, width=width, height=height, saved_states=0, flipped=flipped)
            self.surfaces[view] = result
            return result
        except Exception:
            for pointer in allocated:
                r.allocations.remove(pointer)
                release(r.worker, pointer)
            if native_context:
                r.bridge.message(native_context, 'release')
            self.native.CGContextRelease(context)
            raise

    def set_current(self, context):
        bridge = self.registry.bridge
        send = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)(('objc_msgSend', bridge.native))
        send(bridge.native.objc_getClass(b'NSGraphicsContext'), bridge.native.sel_registerName(b'setCurrentContext:'), context)

    def lock(self, view):
        if len(self.stack) >= 32:
            raise CompatibilityError('Editor focus nesting exceeds limit')
        surface = self.surface(view)
        bridge = self.registry.bridge
        previous = bridge.message(bridge.native.objc_getClass(b'NSGraphicsContext'), 'currentContext')
        if previous:
            bridge.message(previous, 'retain')
        self.set_current(surface['native'])
        self.stack.append((view, previous))
        return 0

    def unlock(self, view):
        if not self.stack or self.stack[-1][0] != view:
            raise CompatibilityError('Unbalanced editor focus unlock')
        _, previous = self.stack.pop()
        self.set_current(previous)
        if previous:
            self.registry.bridge.message(previous, 'release')
        return 0

    def message(self, receiver, selector, is_class):
        if is_class and selector == 'currentContext':
            return self.surfaces[self.stack[-1][0]]['pointer'] if self.stack else 0
        for surface in self.surfaces.values():
            if surface['pointer'] == receiver:
                if selector in ('graphicsPort', 'CGContext'):
                    return surface['port']
                if selector == 'isFlipped':
                    return int(surface['flipped'])
                if selector == 'isDrawingToScreen':
                    return 0
        raise CompatibilityError(f'Unsupported bitmap NSGraphicsContext message {selector}')

    def discard(self, view):
        if any(v == view for v, _ in self.stack):
            raise CompatibilityError('Cannot destroy a focused editor view')
        surface = self.surfaces.pop(view, None)
        if surface:
            r = self.registry
            r.bridge.message(surface['native'], 'release')
            self.native.CGContextRelease(surface['context'])
            del r.instances[surface['pointer']]
            for pointer in (surface['pointer'], surface['port']):
                r.allocations.remove(pointer)
                release(r.worker, pointer)

    def png(self, view):
        """Encode offscreen pixels; this never captures a screen/window."""
        import zlib
        surface = self.surfaces[view]
        width, height = surface['width'], surface['height']
        pixels = bytearray(surface['pixels'].raw)
        alphas = pixels[3::4]
        nontransparent = len(alphas) - alphas.count(0)
        # PNG stores straight alpha, while the drawing buffer is premultiplied.
        if alphas.count(255) != len(alphas):
            for offset in range(0, len(pixels), 4):
                alpha = pixels[offset + 3]
                if 0 < alpha < 255:
                    for channel in range(3):
                        pixels[offset + channel] = min(255, (pixels[offset + channel] * 255 + alpha // 2) // alpha)
        def chunk(kind, data):
            return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)
        # CoreGraphics bitmap storage already orders rows from the image top.
        rows = b''.join(b'\0' + pixels[y * width * 4:(y + 1) * width * 4] for y in range(height))
        data = (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>2I5B', width, height, 8, 6, 0, 0, 0))
                + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))
        return data, {'width': width, 'height': height, 'nontransparent_pixels': nontransparent}

    def export_png(self, view, path):
        from pathlib import Path
        data, details = self.png(view)
        Path(path).write_bytes(data)
        return {'path': str(path), **details}

    def close(self):
        while self.stack:
            self.unlock(self.stack[-1][0])
        for view in list(self.surfaces):
            self.discard(view)
