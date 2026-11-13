"""Explicit missing-resource results for bundles without classic resource files."""
from pathlib import Path

from macho import CompatibilityError

SYMBOLS = ('_GetResource', '_ResError', '_CurResFile', '_UseResFile', '_StandardAlert')


class Resources32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.error = 0
        self.queries = []
        self.alerts = []
        binary = getattr(runtime.image, 'path', None)
        self.classic_files = []
        if binary:
            resources = Path(binary).parent.parent / 'Resources'
            self.classic_files = list(resources.glob('*.rsrc'))

    def invoke(self, name, arg):
        if name == '_StandardAlert':
            messages = []
            for index in (1, 2):
                pointer = arg(index)
                size = self.runtime.read(pointer, 1)[0] if pointer else 0
                messages.append(self.runtime.read(pointer + 1, size).decode('mac_roman') if size else '')
            if len(self.alerts) < 32:
                self.alerts.append(messages)
            if arg(4):
                self.runtime.cpu.mem_write(arg(4), bytes(2))
            return -128  # userCanceledErr; no modal UI exists in the offline worker
        if name == '_ResError':
            return self.error
        if name == '_CurResFile':
            return 0  # empty guest system resource map
        if name == '_UseResFile':
            self.error = 0 if arg(0) & 0xffff == 0 else -193  # resFNotFound
            return 0
        typ, ident = arg(0), arg(1) & 0xffff
        ident = ident if ident < 0x8000 else ident - 0x10000
        if len(self.queries) < 128:
            self.queries.append(dict(type=typ.to_bytes(4, 'big').decode('ascii', errors='replace'), id=ident))
        if self.classic_files:
            raise CompatibilityError('Classic resource-file decoding is not implemented on i386')
        # This guest resource chain is empty. GetResource specifies NIL and
        # resNotFound; image files in Resources are loaded through other APIs.
        self.error = -192
        return 0
