"""Owned native color-space objects behind opaque 32-bit guest tokens."""
import ctypes
import struct

from darwin32 import CallBatch
from libc_shims import release

from macho import CompatibilityError

SYMBOLS = ('_CGColorSpaceCreateDeviceRGB', '_CGColorSpaceRelease',
           '_CGImageGetWidth', '_CGImageGetHeight', '_CGImageRetain', '_CGImageRelease',
           '_CGDataProviderCreate', '_CGDataProviderCreateWithData', '_CGDataProviderRelease', '_CGImageCreate')


class Graphics32:
    def __init__(self, runtime=None):
        self.runtime = runtime
        self.library = None
        self.objects = {}
        self.next_token = 0x75000000
        self.images = {}
        self.next_image = 0x75e00000
        self.imageio = None
        self.providers = {}
        self.next_provider = 0x76100000

    def prepare(self):
        if self.library is not None:
            return
        self.library = ctypes.CDLL('/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics')
        self.library.CGColorSpaceCreateDeviceRGB.argtypes = []
        self.library.CGColorSpaceCreateDeviceRGB.restype = ctypes.c_void_p
        self.library.CGColorSpaceRelease.argtypes = [ctypes.c_void_p]
        self.library.CGColorSpaceRelease.restype = None
        for name in ('CGImageGetWidth', 'CGImageGetHeight'):
            function = getattr(self.library, name)
            function.argtypes, function.restype = [ctypes.c_void_p], ctypes.c_size_t
        self.library.CGImageRelease.argtypes = [ctypes.c_void_p]
        self.library.CGImageRelease.restype = None
        self.library.CGDataProviderCreateWithData.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
        self.library.CGDataProviderCreateWithData.restype = ctypes.c_void_p
        self.library.CGDataProviderRelease.argtypes = [ctypes.c_void_p]
        self.library.CGDataProviderRelease.restype = None
        self.library.CGImageCreate.argtypes = [ctypes.c_size_t] * 5 + [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint8, ctypes.c_int]
        self.library.CGImageCreate.restype = ctypes.c_void_p

    def load_image(self, path):
        self.prepare()
        if path.stat().st_size > 16 * 1024 * 1024 or len(self.images) >= 256 or self.next_image >= 0x75f00000:
            raise CompatibilityError('i386 image resource limit exceeded')
        if self.imageio is None:
            self.imageio = ctypes.CDLL('/System/Library/Frameworks/ImageIO.framework/ImageIO')
            self.cf = ctypes.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
            signatures = {
                'CFURLCreateFromFileSystemRepresentation': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_long, ctypes.c_uint8]),
                'CFRelease': (None, [ctypes.c_void_p]),
                'CFDictionaryGetValue': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
                'CFNumberGetValue': (ctypes.c_uint8, [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]),
                'CGImageSourceCreateWithURL': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
                'CGImageSourceCopyPropertiesAtIndex': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]),
                'CGImageSourceCreateImageAtIndex': (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p])}
            for name, (result, args) in signatures.items():
                function = getattr(self.cf if name.startswith('CF') else self.imageio, name)
                function.argtypes, function.restype = args, result
        data = str(path).encode('utf-8')
        url = self.cf.CFURLCreateFromFileSystemRepresentation(None, data, len(data), False)
        source = properties = None
        try:
            if not url:
                return 0
            source = self.imageio.CGImageSourceCreateWithURL(url, None)
            if not source:
                return 0
            properties = self.imageio.CGImageSourceCopyPropertiesAtIndex(source, 0, None)
            if not properties:
                return 0
            sizes = []
            for name in ('kCGImagePropertyPixelWidth', 'kCGImagePropertyPixelHeight'):
                key = ctypes.c_void_p.in_dll(self.imageio, name).value
                number = self.cf.CFDictionaryGetValue(properties, key)
                value = ctypes.c_int64()
                if not number or not self.cf.CFNumberGetValue(number, 4, ctypes.byref(value)):
                    raise CompatibilityError('Image has no bounded dimensions')
                sizes.append(value.value)
            width, height = sizes
            if not (1 <= width <= 4096 and 1 <= height <= 4096) or width * height > 4 * 1024 * 1024:
                raise CompatibilityError('i386 image dimensions exceed limit')
            if sum(v['width'] * v['height'] for v in self.images.values()) + width * height > 16 * 1024 * 1024:
                raise CompatibilityError('i386 image pixel budget exceeded')
            image = self.imageio.CGImageSourceCreateImageAtIndex(source, 0, None)
            if not image:
                return 0
            if (self.library.CGImageGetWidth(image), self.library.CGImageGetHeight(image)) != (width, height):
                self.library.CGImageRelease(image)
                raise CompatibilityError('Decoded image dimensions differ from bounded metadata')
            token = self.next_image
            self.next_image += 16
            self.images[token] = dict(pointer=image, refs=1, width=width, height=height)
            return token
        finally:
            for pointer in (properties, source, url):
                if pointer:
                    self.cf.CFRelease(pointer)

    def release_provider(self, token):
        record = self.providers.get(token)
        if record is None:
            raise CompatibilityError('Unknown i386 data provider')
        record['refs'] -= 1
        if record['refs']:
            return 0
        del self.providers[token]
        if record['pointer']:
            self.library.CGDataProviderRelease(record['pointer'])
        callback = record.get('callbacks', (0, 0, 0, 0))[3]
        if callback:
            return CallBatch([(callback, (record['info'],))])
        return 0

    def invoke(self, name, arg):
        self.prepare()
        if name == '_CGDataProviderCreate':
            if len(self.providers) >= 256 or self.next_provider >= 0x76200000:
                raise CompatibilityError('i386 data-provider limit exceeded')
            callbacks = struct.unpack('<4I', self.runtime.read(arg(1), 16))
            if not callbacks[0]:
                raise CompatibilityError('Sequential provider requires getBytes')
            token = self.next_provider
            self.next_provider += 16
            self.providers[token] = dict(pointer=0, storage=None, size=0, refs=1,
                                         info=arg(0), callbacks=callbacks)
            return token
        if name == '_CGDataProviderCreateWithData':
            if arg(3):
                raise CompatibilityError('i386 data-provider release callback not implemented')
            size = arg(2)
            if size > 16 * 1024 * 1024 or len(self.providers) >= 256 or self.next_provider >= 0x76200000:
                raise CompatibilityError('i386 data-provider limit exceeded')
            if sum(record['size'] for record in self.providers.values()) + size > 32 * 1024 * 1024:
                raise CompatibilityError('i386 provider byte budget exceeded')
            storage = ctypes.create_string_buffer(self.runtime.read(arg(1), size), max(1, size))
            pointer = self.library.CGDataProviderCreateWithData(None, storage, size, None)
            if not pointer:
                return 0
            token = self.next_provider
            self.next_provider += 16
            self.providers[token] = dict(pointer=pointer, storage=storage, size=size, refs=1)
            return token
        if name == '_CGDataProviderRelease':
            return self.release_provider(arg(0))
        if name == '_CGImageCreate':
            width, height, bits, pixel_bits, stride, color, flags, provider, decode, interpolate, intent = (arg(i) for i in range(11))
            if not (1 <= width <= 4096 and 1 <= height <= 4096) or width * height > 4 * 1024 * 1024:
                raise CompatibilityError('Invalid i386 image dimensions')
            if bits != 8 or pixel_bits != 32 or stride < width * 4 or stride * height > 16 * 1024 * 1024 or decode:
                raise CompatibilityError('Unsupported i386 image layout/decode array')
            source, space = self.providers.get(provider), self.objects.get(color)
            if source is None or space is None:
                raise CompatibilityError('Invalid i386 image provider/color space')
            if len(self.images) >= 256 or self.next_image >= 0x75f00000 or sum(v['width'] * v['height'] for v in self.images.values()) + width * height > 16 * 1024 * 1024:
                raise CompatibilityError('i386 image pixel budget exceeded')
            def create_image():
                if source['size'] < stride * height:
                    raise CompatibilityError('Image provider returned insufficient data')
                pointer = self.library.CGImageCreate(width, height, bits, pixel_bits, stride, space, flags,
                                                      source['pointer'], None, bool(interpolate), intent)
                if not pointer:
                    return 0
                token = self.next_image
                self.next_image += 16
                source['refs'] += 1
                self.images[token] = dict(pointer=pointer, refs=1, width=width, height=height,
                                          storage=source['storage'], provider=provider)
                return token
            if source.get('callbacks') and not source['pointer']:
                size = stride * height
                if sum(record['size'] for record in self.providers.values()) + size > 32 * 1024 * 1024:
                    raise CompatibilityError('i386 provider byte budget exceeded')
                destination = self.runtime.allocate(size)
                get_bytes, skip, rewind, dispose = source['callbacks']
                calls = [(rewind, (source['info'],))] if rewind else []
                calls.append((get_bytes, (source['info'], destination, size)))
                def finish(results):
                    try:
                        if results[-1] != size:
                            raise CompatibilityError('Sequential image provider returned a short read')
                        storage = ctypes.create_string_buffer(self.runtime.read(destination, size), size)
                        pointer = self.library.CGDataProviderCreateWithData(None, storage, size, None)
                        if not pointer:
                            return 0
                        source.update(pointer=pointer, storage=storage, size=size)
                        return create_image()
                    finally:
                        release(self.runtime, destination)
                return CallBatch(calls, finish)
            return create_image()
        if name.startswith('_CGImage'):
            token = arg(0)
            if token not in self.images:
                raise CompatibilityError('Unknown i386 image token')
            image = self.images[token]
            if name == '_CGImageGetWidth':
                return image['width']
            if name == '_CGImageGetHeight':
                return image['height']
            if name == '_CGImageRetain':
                image['refs'] += 1
                return token
            image['refs'] -= 1
            if not image['refs']:
                self.library.CGImageRelease(image['pointer'])
                del self.images[token]
                if 'provider' in image:
                    return self.release_provider(image['provider'])
            return 0
        if name == '_CGColorSpaceCreateDeviceRGB':
            if len(self.objects) >= 256 or self.next_token >= 0x75100000:
                raise CompatibilityError('i386 color-space object limit exceeded')
            pointer = self.library.CGColorSpaceCreateDeviceRGB()
            if not pointer:
                return 0
            token = self.next_token
            self.next_token += 16
            self.objects[token] = pointer
            return token
        token = arg(0)
        if token:
            pointer = self.objects.pop(token, None)
            if pointer is None:
                raise CompatibilityError('Unknown i386 color-space token')
            self.library.CGColorSpaceRelease(pointer)
        return 0

    def close(self):
        if self.library is not None:
            for image in self.images.values():
                self.library.CGImageRelease(image['pointer'])
            self.images.clear()
            for provider in self.providers.values():
                if provider['pointer']:
                    self.library.CGDataProviderRelease(provider['pointer'])
            self.providers.clear()
            for pointer in self.objects.values():
                self.library.CGColorSpaceRelease(pointer)
            self.objects.clear()
