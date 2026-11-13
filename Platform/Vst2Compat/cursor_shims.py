"""Cursor requests for the offscreen editor; never mutate the system cursor."""
from macho import CompatibilityError


class Cursors:
    def __init__(self, registry):
        self.registry = registry
        self.records = {}
        self.styles = {}
        self.current = 0

    def factory(self, cls, selector):
        if selector != 'arrowCursor':
            raise CompatibilityError(f'Unsupported offscreen cursor factory {selector}')
        if selector not in self.styles:
            pointer = self.registry.allocate(16)
            self.registry.worker.write_u64(pointer, cls.pointer)
            self.registry.instances[pointer] = [cls.pointer, 1]
            self.records[pointer] = 'arrow'
            self.styles[selector] = pointer
        return self.styles[selector]

    def message(self, pointer, selector):
        if selector == 'set':
            # No visible window exists. Publish the requested style for a future
            # editor surface instead of altering the user's global mouse cursor.
            self.current = pointer
            return 0
        raise CompatibilityError(f'Unsupported offscreen cursor message {selector}')

    def requested_style(self):
        return self.records.get(self.current)

    def close(self):
        self.records.clear()
        self.styles.clear()
        self.current = 0
