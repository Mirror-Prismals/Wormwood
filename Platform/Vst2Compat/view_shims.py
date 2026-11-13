"""Opt-in offscreen NSView backing for editor diagnostics, never Intel native IMPs."""
import ctypes
import math
import struct
from contextlib import contextmanager
from macho import CompatibilityError


class Point(ctypes.Structure):
    _fields_ = [('x', ctypes.c_double), ('y', ctypes.c_double)]


class Size(ctypes.Structure):
    _fields_ = [('width', ctypes.c_double), ('height', ctypes.c_double)]


class Rect(ctypes.Structure):
    _fields_ = [('origin', Point), ('size', Size)]


class Views:
    def __init__(self, registry):
        self.registry = registry
        self.records = {}

    def allocate(self, cls):
        registry = self.registry
        bridge = registry.bridge
        registry.native_setup()
        native = bridge.message(bridge.native.objc_getClass(b'NSView'), 'alloc')
        if not native:
            raise CompatibilityError('Native NSView allocation failed')
        try:
            pointer = registry.allocate((cls.size + 7) & -8)
        except Exception:
            bridge.message(native, 'release')
            raise
        registry.worker.write_u64(pointer, cls.pointer)
        registry.instances[pointer] = [cls.pointer, 1]
        self.records[pointer] = {'native': native, 'initialized': False, 'frame': None, 'parent': 0, 'dirty_rect': None}
        return pointer

    def initialize(self, pointer, frame):
        if len(frame) != 4 or not all(math.isfinite(v) and abs(v) <= 1e6 for v in frame) or not all(0 <= v <= 16384 for v in frame[2:]):
            raise CompatibilityError('Unsupported editor view dimensions')
        record = self.records[pointer]
        if record['initialized']:
            raise CompatibilityError('Editor view initialized twice')
        native = self.registry.bridge.message_args(record['native'], 'initWithFrame:', [Rect],
                                                   Rect(Point(*frame[:2]), Size(*frame[2:])))
        record['native'] = native
        if not native:
            raise CompatibilityError('Native NSView initialization failed')
        record.update(initialized=True, frame=list(frame))
        return pointer

    def message(self, pointer, selector):
        registry, w = self.registry, self.registry.worker
        record = self.records[pointer]
        if selector in ('init', 'initWithFrame:'):
            # AMD64 passes a 32-byte CGRect by value on the stack. ARM64 ctypes
            # handles the different native struct ABI; guest pointers never cross.
            frame = (0., 0., 0., 0.) if selector == 'init' else struct.unpack(
                '<4d', w.read(w.cpu.reg_read(w.x86.UC_X86_REG_RSP) + 8, 32))
            return self.initialize(pointer, frame)
        if not record['initialized']:
            raise CompatibilityError('Editor view used before initialization')
        if selector == 'lockFocus':
            return registry.graphics.lock(pointer)
        if selector == 'unlockFocus':
            return registry.graphics.unlock(pointer)
        if selector == 'getRectsBeingDrawn:count:':
            rectangles = record.get('drawing_rect', 0)
            w.write_u64(w.arg(2), rectangles)
            w.write_u64(w.arg(3), 1 if rectangles else 0)
            return 0
        if selector == 'isFlipped':
            return int(record.get('flipped', False))
        if selector == 'window':
            return registry.windows.for_view(pointer)
        if selector in ('mouseDown:', 'mouseUp:', 'mouseDragged:'):
            event = w.arg(2)
            if event not in registry.events.records:
                raise CompatibilityError('Responder forwarding requires a diagnostic event')
            # Guest overrides are selected by Registry before reaching this
            # builtin implementation. NSResponder's default forwards upwards;
            # never send the emulated event to native AppKit.
            if record['parent']:
                return registry.dispatch(record['parent'], selector)
            window = registry.windows.for_view(pointer)
            if window:
                return registry.windows.message(window, selector)
            return 0
        if selector == 'convertPoint:fromView:':
            source = w.arg(2)
            values = [struct.unpack('<d', struct.pack('<Q', w.cpu.reg_read(
                getattr(w.x86, f'UC_X86_REG_XMM{i}')) & ((1 << 64) - 1)))[0] for i in range(2)]
            if not all(math.isfinite(v) and abs(v) <= 1e6 for v in values):
                raise CompatibilityError('Invalid editor point')
            if source:
                values = self.window_point(source, values)
            values = self.local_point(pointer, values)
            for i, value in enumerate(values):
                w.cpu.reg_write(getattr(w.x86, f'UC_X86_REG_XMM{i}'), struct.unpack('<Q', struct.pack('<d', value))[0])
            return 0
        if selector == 'setNeedsDisplayInRect:':
            frame = struct.unpack('<4d', w.read(w.cpu.reg_read(w.x86.UC_X86_REG_RSP) + 8, 32))
            if not all(math.isfinite(v) and abs(v) <= 1e6 for v in frame) or not all(0 <= v <= 16384 for v in frame[2:]):
                raise CompatibilityError('Unsupported editor invalidation rectangle')
            bridge = registry.bridge
            send = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p, Rect)(('objc_msgSend', bridge.native))
            send(record['native'], bridge.native.sel_registerName(selector.encode()),
                 Rect(Point(*frame[:2]), Size(*frame[2:])))
            if frame[2] and frame[3]:
                old = record['dirty_rect']
                if old is None:
                    record['dirty_rect'] = list(frame)
                else:
                    x, y = min(old[0], frame[0]), min(old[1], frame[1])
                    record['dirty_rect'] = [x, y, max(old[0] + old[2], frame[0] + frame[2]) - x,
                                           max(old[1] + old[3], frame[1] + frame[3]) - y]
            return 0
        integer_settings = {'setFocusRingType:': 2, 'setAutoresizingMask:': 63}
        boolean_settings = ('setHidden:', 'setNeedsDisplay:', 'setAutoresizesSubviews:',
                            'setPostsFrameChangedNotifications:', 'setPostsBoundsChangedNotifications:')
        if selector in integer_settings or selector in boolean_settings:
            value = w.arg(2)
            boolean = selector in boolean_settings
            if value > (1 if boolean else integer_settings[selector]):
                raise CompatibilityError(f'Invalid view setting {selector}')
            bridge = registry.bridge
            send = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p,
                                    ctypes.c_bool if boolean else ctypes.c_uint64)(('objc_msgSend', bridge.native))
            send(record['native'], bridge.native.sel_registerName(selector.encode()), value)
            return 0
        if selector in ('focusRingType', 'autoresizingMask', 'isHidden', 'needsDisplay', 'autoresizesSubviews'):
            bridge = registry.bridge
            result_type = ctypes.c_uint64 if selector in ('focusRingType', 'autoresizingMask') else ctypes.c_bool
            send = ctypes.CFUNCTYPE(result_type, ctypes.c_void_p, ctypes.c_void_p)(('objc_msgSend', bridge.native))
            return int(send(record['native'], bridge.native.sel_registerName(selector.encode())))
        if selector == 'registerForDraggedTypes:':
            arrays = registry.arrays
            types = w.arg(2)
            if arrays is None or types not in arrays.records:
                raise CompatibilityError('Drag types require a bridged string NSArray')
            registry.bridge.message_args(record['native'], selector, [ctypes.c_void_p], arrays.records[types]['native'])
            return 0
        if selector == 'unregisterDraggedTypes':
            registry.bridge.message(record['native'], selector)
            return 0
        if selector in ('addTrackingArea:', 'removeTrackingArea:'):
            tracking = registry.tracking
            area = w.arg(2)
            if tracking is None or area not in tracking.records or not tracking.records[area]['initialized']:
                raise CompatibilityError('View tracking requires an initialized bridged area')
            area_record = tracking.records[area]
            adding = selector == 'addTrackingArea:'
            if (adding and area_record['view']) or (not adding and area_record['view'] != pointer):
                raise CompatibilityError('Tracking area attachment mismatch')
            registry.bridge.message_args(record['native'], selector, [ctypes.c_void_p], area_record['native'])
            area_record['view'] = pointer if adding else 0
            if adding:
                registry.instances[area][1] += 1
            else:
                tracking.message(area, 'release')
            return 0
        if selector == 'addSubview:':
            child = w.arg(2)
            if child not in self.records or not self.records[child]['initialized']:
                raise CompatibilityError('Subview must be an initialized bridged view')
            ancestor = pointer
            while ancestor:
                if ancestor == child:
                    raise CompatibilityError('Editor view hierarchy cycle')
                ancestor = self.records[ancestor]['parent']
            child_record = self.records[child]
            registry.bridge.message_args(record['native'], selector, [ctypes.c_void_p], child_record['native'])
            if not child_record['parent']:
                registry.instances[child][1] += 1  # superview owns a reference
            child_record['parent'] = pointer
            return 0
        if selector == 'removeFromSuperview':
            if record['parent']:
                registry.bridge.message(record['native'], selector)
                record['parent'] = 0
                return self.message(pointer, 'release')
            return 0
        if selector == 'superview':
            return record['parent']
        if selector == 'dealloc':
            if not record.get('deallocating'):
                raise CompatibilityError('Editor view dealloc without final release')
            return self.destroy(pointer)
        if selector in ('retain', 'release'):
            if record.get('deallocating'):
                raise CompatibilityError('Editor view reference change during dealloc')
            references = registry.instances[pointer][1]
            if selector == 'retain':
                registry.instances[pointer][1] += 1
                return pointer
            if references == 1:
                cls = registry.instances[pointer][0]
                implementation = registry.inherited(cls, 'methods', 'dealloc')
                record['deallocating'] = True
                if implementation:
                    # Resume the Intel destructor in Unicorn. Its super dealloc
                    # message performs native backing cleanup only after guest
                    # ivars/resources have been disposed of by the plugin.
                    from objc_registry import TailCall
                    return TailCall(implementation[0], pointer, registry.intern('dealloc'))
                return self.destroy(pointer)
            else:
                registry.instances[pointer][1] -= 1
            return 0
        raise CompatibilityError(f'Offscreen NSView bridge does not implement {selector}')

    def destroy(self, pointer):
        registry = self.registry
        record = self.records[pointer]
        registry.graphics.discard(pointer)
        if record['parent'] or any(v['parent'] == pointer for v in self.records.values()):
            raise CompatibilityError('Editor view deallocation requires detached children')
        tracking = registry.tracking
        if any(v.get('owner') == pointer and v['view'] not in (0, pointer) for v in tracking.records.values()):
            raise CompatibilityError('Deallocating editor view still owns another view tracking area')
        for area, area_record in list(tracking.records.items()):
            if area_record['view'] == pointer:
                registry.bridge.message_args(record['native'], 'removeTrackingArea:',
                                             [ctypes.c_void_p], area_record['native'])
                area_record['view'] = 0
                tracking.message(area, 'release')
            if area in tracking.records and area_record.get('owner') == pointer:
                # No event may target a destroyed guest object. The tracking
                # bridge pins the plain native owner until pending releases run.
                area_record['owner'] = 0
        registry.bridge.message(record['native'], 'release')
        registry.windows.detach_view(pointer)
        del self.records[pointer]
        del registry.instances[pointer]
        registry.allocations.remove(pointer)
        from libc_shims import release
        release(registry.worker, pointer)
        return 0

    def return_rect(self, destination, pointer, selector):
        registry = self.registry
        if pointer not in self.records or not self.records[pointer]['initialized']:
            raise CompatibilityError('Rectangle query requires an initialized bridged view')
        if selector not in ('frame', 'bounds', 'visibleRect'):
            raise CompatibilityError(f'Unsupported structure-return view query {selector}')
        cls = registry.instances[pointer][0]
        if registry.inherited(cls, 'methods', selector):
            raise CompatibilityError('Guest structure-return view overrides are not implemented')
        bridge = registry.bridge
        send = ctypes.CFUNCTYPE(Rect, ctypes.c_void_p, ctypes.c_void_p)(('objc_msgSend', bridge.native))
        rect = send(self.records[pointer]['native'], bridge.native.sel_registerName(selector.encode()))
        registry.worker.cpu.mem_write(destination, struct.pack('<4d', rect.origin.x, rect.origin.y,
                                                              rect.size.width, rect.size.height))
        return destination

    def probe_draw(self, pointer):
        """Enter guest drawing in a disposable worker; no native IMP registration.

        Bitmap focus is available, but unsupported guest graphics operations
        still trap. Nonempty pixels do not establish visual correctness.
        """
        registry, w = self.registry, self.registry.worker
        record = self.records.get(pointer)
        if not record or not record['initialized'] or record.get('deallocating'):
            raise CompatibilityError('Drawing requires a live initialized editor view')
        cls = registry.instances[pointer][0]
        if not registry.inherited(cls, 'methods', 'drawRect:'):
            raise CompatibilityError('Editor view does not implement drawRect:')
        # Cocoa establishes the view's coordinate orientation before drawing.
        # Query guest code through Unicorn, never through the native NSView.
        record['flipped'] = bool(w.call(w.shim_address('_objc_msgSend'), pointer, registry.intern('isFlipped'))
                                 if registry.inherited(cls, 'methods', 'isFlipped') else False)
        width, height = record['frame'][2:]
        rectangle = (0., 0., width, height)
        bits = struct.unpack('<4Q', struct.pack('<4d', *rectangle))
        with self.drawing(pointer, rectangle):
            w.call(w.shim_address('_objc_msgSend'), pointer, registry.intern('drawRect:'),
                   0, 0, 0, 0, *bits)
        surface = registry.graphics.surfaces.get(pointer)
        return {'guest_pointer': pointer, 'rectangle': list(rectangle),
                'callback_completed': True,
                'pixels_rendered': bool(surface and any(surface['pixels'].raw[3::4]))}

    @contextmanager
    def drawing(self, pointer, rectangle):
        record = self.records[pointer]
        if record.get('drawing_rect'):
            raise CompatibilityError('Recursive view drawing is not supported')
        registry = self.registry
        storage = registry.allocate(32)
        try:
            registry.worker.cpu.mem_write(storage, struct.pack('<4d', *rectangle))
            record['drawing_rect'] = storage
            yield
        finally:
            record.pop('drawing_rect', None)
            registry.allocations.remove(storage)
            from libc_shims import release
            release(registry.worker, storage)

    def point_chain(self, pointer):
        chain = []
        while pointer:
            record = self.records.get(pointer)
            if not record or not record['initialized'] or record.get('deallocating'):
                raise CompatibilityError('Point conversion requires a live editor view')
            chain.append(record)
            pointer = record['parent']
        return chain

    def window_point(self, pointer, point):
        x, y = point
        for record in self.point_chain(pointer):
            parent = self.records.get(record['parent'], {})
            fx, fy, _, height = record['frame']
            if record.get('flipped', False) != parent.get('flipped', False):
                y = height - y
            x, y = x + fx, y + fy
        return x, y

    def local_point(self, pointer, point):
        x, y = point
        for record in reversed(self.point_chain(pointer)):
            parent = self.records.get(record['parent'], {})
            fx, fy, _, height = record['frame']
            x, y = x - fx, y - fy
            if record.get('flipped', False) != parent.get('flipped', False):
                y = height - y
        return x, y

    def probe_click(self, pointer, x, y):
        """Click a view-local point in a disposable worker only."""
        self.probe_mouse(pointer, [(1, 'mouseDown:', x, y), (2, 'mouseUp:', x, y)])

    def probe_drag(self, pointer, x, y, end_x, end_y):
        points = [(1, 'mouseDown:', x, y)]
        points.extend((6, 'mouseDragged:', x + (end_x - x) * i / 8, y + (end_y - y) * i / 8)
                      for i in range(1, 9))
        points.append((2, 'mouseUp:', end_x, end_y))
        self.probe_mouse(pointer, points)

    def probe_mouse(self, pointer, points):
        import time
        r, w = self.registry, self.registry.worker
        record = self.records[pointer]
        cls = r.instances[pointer][0]
        # Validate the complete gesture before delivering its first event.
        for _, selector, x, y in points:
            if not (math.isfinite(x) and math.isfinite(y) and 0 <= x < record['frame'][2] and 0 <= y < record['frame'][3]):
                raise CompatibilityError('Diagnostic mouse point is outside the editor')
            if not r.inherited(cls, 'methods', selector):
                raise CompatibilityError(f'Editor does not implement {selector}')
        for kind, selector, x, y in points:
            location = self.window_point(pointer, (x, y))
            timestamp = time.monotonic()
            event = r.events.create(kind, location, timestamp, r.windows.for_view(pointer))
            try:
                w.call(w.shim_address('_objc_msgSend'), pointer, r.intern(selector), event)
            finally:
                if event in r.events.records:
                    r.events.message(event, 'release')

    def close(self):
        for record in self.records.values():
            if record['native']:
                self.registry.bridge.message(record['native'], 'release')
        self.records.clear()

    def diagnostics(self):
        return [{'guest_pointer': pointer, 'initialized': record['initialized'],
                 'frame': record['frame'], 'parent': record['parent'], 'dirty_rect': record['dirty_rect']}
                for pointer, record in self.records.items()]
