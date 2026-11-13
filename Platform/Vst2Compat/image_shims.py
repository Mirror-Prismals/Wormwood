"""ImageIO resource loading with opaque, owned CF handles; no guest UI execution."""
import ctypes
from collections import deque
from macho import CompatibilityError

SIGNATURES = {
    "CGImageSourceCreateWithURL": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]),
    "CGImageSourceCreateImageAtIndex": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]),
    "CGImageSourceCopyPropertiesAtIndex": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]),
    "CGImageGetWidth": (ctypes.c_size_t, [ctypes.c_void_p]),
    "CGImageGetHeight": (ctypes.c_size_t, [ctypes.c_void_p]),
    "CGImageGetBitsPerComponent": (ctypes.c_size_t, [ctypes.c_void_p]),
}
SYMBOLS = tuple("_" + name for name in SIGNATURES) + ("_CGImageRelease", "_CGImageRetain")


class Images:
    def __init__(self, worker):
        self.worker = worker
        self.native = None
        self.queries = deque(maxlen=64)

    def invoke(self, symbol):
        w, name = self.worker, symbol[1:]
        a, b, c = (w.arg(i) for i in range(3))
        if self.native is None:
            self.native = ctypes.CDLL("/System/Library/Frameworks/ImageIO.framework/ImageIO")
            self.graphics = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
            for api, (result, args) in SIGNATURES.items():
                library = self.native if api.startswith("CGImageSource") else self.graphics
                function = getattr(library, api)
                function.restype, function.argtypes = result, args
        w.cf.load_native()
        if name in ("CGImageRelease", "CGImageRetain"):
            w.cf.get(a, "image")
            return w.cf.invoke("_CFRelease" if name.endswith("Release") else "_CFRetain")
        if name.startswith("CGImageGet"):
            return getattr(self.graphics, name)(w.cf.get(a, "image"))
        if name == "CGImageSourceCreateWithURL":
            options = w.cf.get(b, "dictionary") if b else None
            url = w.cf.get(a, "url")
            scheme_function = w.cf.native.CFURLCopyScheme
            scheme_function.argtypes, scheme_function.restype = [ctypes.c_void_p], ctypes.c_void_p
            scheme = scheme_function(url)
            try:
                value = ctypes.create_string_buffer(16)
                if not scheme or not w.cf.native.CFStringGetCString(scheme, value, len(value), 0x08000100) or value.value.lower() != b"file":
                    raise CompatibilityError("Only local file image URLs are supported")
            finally:
                if scheme:
                    w.cf.native.CFRelease(scheme)
            path = ctypes.create_string_buffer(65536)
            if not w.cf.native.CFURLGetFileSystemRepresentation(url, True, path, len(path)):
                raise CompatibilityError("Only local file image URLs are supported")
            resolved = w.resource_paths.resolve(path.value)
            mapped_url = None
            try:
                if resolved != path.value:
                    mapped_url = w.cf.native.CFURLCreateFromFileSystemRepresentation(None, resolved, len(resolved), False)
                    if not mapped_url:
                        raise CompatibilityError("Cannot create mapped image resource URL")
                native = self.native.CGImageSourceCreateWithURL(mapped_url or url, options)
            finally:
                if mapped_url:
                    w.cf.native.CFRelease(mapped_url)
            self.queries.append({"path": path.value.decode("utf-8", errors="replace"), "found": bool(native)})
            return w.cf.wrap(native, "image_source")
        options = w.cf.get(c, "dictionary") if c else None
        source = w.cf.get(a, "image_source")
        kind = "image" if name == "CGImageSourceCreateImageAtIndex" else "dictionary"
        return w.cf.wrap(getattr(self.native, name)(source, b, options), kind)
