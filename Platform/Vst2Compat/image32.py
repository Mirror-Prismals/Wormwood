"""Legacy resource-file identities and image import for i386 editor workers."""
import ctypes
import struct
from pathlib import Path
from macho import CompatibilityError

SYMBOLS = ('_FSGetCatalogInfo', '_GetGraphicsImporterForFile', '_GraphicsImportSetDataFile',
           '_GraphicsImportCreateCGImage', '_GraphicsImportGetSourceRect', '_CloseComponent',
           '_GraphicsImportSetGWorld', '_GraphicsImportDraw')


class Images32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.specs = {}
        self.directories = {}
        self.importers = {}
        self.next_importer = 0x75d00000

    def file(self, pointer):
        spec = self.runtime.read(pointer, 70)
        if spec not in self.specs:
            raise CompatibilityError('Unknown guest FSSpec')
        return self.specs[spec]

    def invoke(self, name, arg):
        w = self.runtime
        if name == '_FSGetCatalogInfo':
            if arg(1) or arg(2) or arg(3) or arg(5):
                raise CompatibilityError('i386 catalog metadata beyond FSSpec is not implemented')
            if not arg(4):
                return -50
            w.filesystem.load_native()
            path = ctypes.create_string_buffer(4096)
            status = w.filesystem.native.FSRefMakePath(w.filesystem.reference(arg(0)), path, len(path))
            if status:
                return status
            value = Path(path.value.decode('utf-8'))
            try:
                filename = value.name.encode('mac_roman')
            except UnicodeError:
                return -37  # badNameErr: FSSpec names cannot represent this name
            if len(filename) > 63:
                return -37
            if len(self.specs) >= 1024 or len(self.directories) >= 1024:
                raise CompatibilityError('i386 file identity limit exceeded')
            parent = self.directories.setdefault(value.parent, len(self.directories) + 1)
            spec = struct.pack('<hiB63s', 1, parent, len(filename), filename)
            self.specs[spec] = value
            w.cpu.mem_write(arg(4), spec)
            return 0
        if name == '_GetGraphicsImporterForFile':
            path = self.file(arg(0))
            if not arg(1):
                return -50
            if len(self.importers) >= 256 or self.next_importer >= 0x75e00000:
                raise CompatibilityError('i386 graphics importer limit exceeded')
            token = self.next_importer
            self.next_importer += 16
            self.importers[token] = dict(path=path, image=0)
            w.cpu.mem_write(arg(1), struct.pack('<I', token))
            return 0
        token = arg(0)
        if token not in self.importers:
            raise CompatibilityError('Unknown i386 graphics importer')
        if name == '_GraphicsImportSetGWorld':
            if arg(1) not in w.quickdraw.worlds or arg(2):
                raise CompatibilityError('Unsupported i386 graphics-import destination')
            self.importers[token]['world'] = arg(1)
            return 0
        if name == '_GraphicsImportDraw':
            w.quickdraw.draw_image(self.importers[token].get('world'), self.image(token))
            return 0
        if name == '_CloseComponent':
            self.release_image(token)
            del self.importers[token]
            return 0
        if name == '_GraphicsImportSetDataFile':
            path = self.file(arg(1))
            self.release_image(token)
            self.importers[token] = dict(path=path, image=0)
            return 0
        if name == '_GraphicsImportGetSourceRect':
            image = self.image(token)
            if not image:
                return -50
            record = w.graphics.images[image]
            w.cpu.mem_write(arg(1), struct.pack('<4h', 0, 0, record['height'], record['width']))
            return 0
        if name == '_GraphicsImportCreateCGImage':
            if arg(2):
                raise CompatibilityError('Unsupported legacy image import flags')
            image = self.image(token)
            if image:
                w.graphics.invoke('_CGImageRetain', lambda i: image)
            w.cpu.mem_write(arg(1), struct.pack('<I', image))
            return 0 if image else -50
        raise CompatibilityError('Unimplemented i386 image importer operation')

    def image(self, token):
        record = self.importers[token]
        if not record['image']:
            record['image'] = self.runtime.graphics.load_image(record['path'])
        return record['image']

    def release_image(self, token):
        image = self.importers[token]['image']
        if image:
            self.runtime.graphics.invoke('_CGImageRelease', lambda i: image)
