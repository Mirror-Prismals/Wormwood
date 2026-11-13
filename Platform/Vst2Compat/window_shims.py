"""Virtual window identity and focus for isolated editor input diagnostics."""
from macho import CompatibilityError
from libc_shims import release


class Windows:
    def __init__(self, registry):
        self.registry = registry
        self.records = {}

    def attach(self, root):
        r = self.registry
        if root not in r.views.records or r.views.records[root]['parent']:
            raise CompatibilityError('Diagnostic window requires a root view')
        existing = self.for_view(root)
        if existing:
            return existing
        cls = r.builtin('NSWindow')
        pointer = r.allocate(16)
        r.worker.write_u64(pointer, cls)
        r.instances[pointer] = [cls, 1]
        self.records[pointer] = {'root': root, 'responder': 0}
        return pointer

    def for_view(self, view):
        r = self.registry
        while view in r.views.records and r.views.records[view]['parent']:
            view = r.views.records[view]['parent']
        return next((p for p, record in self.records.items() if record['root'] == view), 0)

    def message(self, pointer, selector):
        r, record = self.registry, self.records[pointer]
        if selector == 'makeFirstResponder:':
            responder = r.worker.arg(2)
            if responder and (responder not in r.views.records or self.for_view(responder) != pointer):
                raise CompatibilityError('First responder must belong to the diagnostic window')
            # This models identity only. Native responder notifications and
            # arbitrary responder subclasses remain outside the diagnostic.
            record['responder'] = responder
            return 1
        if selector == 'firstResponder':
            return record['responder']
        if selector == 'contentView':
            return record['root']
        if selector == 'windowNumber':
            return list(self.records).index(pointer) + 1
        if selector in ('isKeyWindow', 'isMainWindow'):
            return 1
        if selector in ('mouseDown:', 'mouseUp:', 'mouseDragged:'):
            event = r.worker.arg(2)
            if event not in r.events.records:
                raise CompatibilityError('Window forwarding requires a diagnostic event')
            # End of the virtual responder chain: record the unhandled event
            # instead of issuing a desktop beep or entering a native event loop.
            record['last_unhandled_event'] = selector
            return 0
        raise CompatibilityError(f'Unsupported diagnostic NSWindow message {selector}')

    def detach_view(self, view):
        for record in self.records.values():
            if record['responder'] == view:
                record['responder'] = 0
            if record['root'] == view:
                record['root'] = 0

    def close(self):
        r = self.registry
        for pointer in self.records:
            del r.instances[pointer]
            r.allocations.remove(pointer)
            release(r.worker, pointer)
        self.records.clear()
