"""Separate editor instance: bitmap frames and parameter changes, never audio."""
import math
import struct
from macho import CompatibilityError
from protocol import MAX_AUDIO
from session import Session


class EditorSession:
    def __init__(self, request):
        self.session = None
        self.opened = False
        self.pool = None
        self.failed = False
        self.mouse_pressed = False
        try:
            self.session = Session({**request, 'experimental': True}, editor_options={'editor_views': True})
            w = self.runtime
            if not w.flags & 1:
                raise CompatibilityError('Plugin does not advertise an original editor')
            if request.get('state') is not None:
                self.session.load_state(request['state'], include_parameters=False)
            self.apply_parameters(request.get('parameters', []))
            r = w.objc.registry
            self.pool = w.call(w.shim_address('_objc_msgSend'), w.objc.class_address, r.intern('new'))
            output = w.scratch(bytes(8))
            w.dispatch(13, pointer=output)
            rect = w.read_u64(output)
            if not rect:
                raise CompatibilityError('Editor did not provide dimensions')
            top, left, bottom, right = struct.unpack('<4h', w.read(rect, 8))
            self.width, self.height = right - left, bottom - top
            if not (1 <= self.width <= 4096 and 1 <= self.height <= 4096) or self.width * self.height > 4 * 1024 * 1024:
                raise CompatibilityError('Editor dimensions exceed bitmap limits')
            self.parent = r.views.allocate(r.require(r.builtin('NSView')))
            r.views.initialize(self.parent, (0., 0., float(self.width), float(self.height)))
            r.windows.attach(self.parent)
            self.opened = bool(w.dispatch(14, pointer=self.parent))
            if not self.opened:
                raise CompatibilityError('Plugin rejected editor open')
            candidates = [p for p, v in r.views.records.items() if v['parent'] == self.parent
                          and r.inherited(r.instances[p][0], 'methods', 'drawRect:')]
            if len(candidates) != 1:
                raise CompatibilityError('Editor requires a single drawable root view')
            self.view = candidates[0]
        except Exception:
            self.failed = True
            self.close()
            raise

    @property
    def runtime(self):
        return self.session.runtime

    def apply_parameters(self, parameters):
        w = self.runtime
        if not isinstance(parameters, list) or len(parameters) > w.params:
            raise CompatibilityError('Invalid editor parameter update list')
        seen = set()
        for item in parameters:
            if not isinstance(item, (list, tuple)) or len(item) != 2:
                raise CompatibilityError('Invalid editor parameter update')
            index, value = item
            if (type(index) is not int or index in seen or not 0 <= index < w.params
                    or type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1):
                raise CompatibilityError('Invalid editor parameter value')
            seen.add(index)
        for index, value in parameters:
            if w.get_parameter(index) != value:
                w.set_parameter(index, value)

    def values(self):
        return [self.runtime.get_parameter(i) for i in range(self.runtime.params)]

    def frame(self):
        w = self.runtime
        w.dispatch(19)
        w.objc.registry.views.probe_draw(self.view)
        payload, details = w.objc.registry.graphics.png(self.view)
        if len(payload) > MAX_AUDIO:
            raise CompatibilityError('Editor PNG exceeds worker frame limit')
        return {'encoding': 'png', **details}, payload

    def gesture(self, action, coordinates):
        count = 2 if action == 'click' else 4 if action == 'drag' else 0
        if not count or not isinstance(coordinates, list) or len(coordinates) != count or any(type(v) not in (int, float) for v in coordinates):
            raise CompatibilityError('Invalid editor mouse gesture')
        before = self.values()
        # Host bitmap coordinates start at the top. A nonflipped guest view
        # instead measures y from its bottom edge.
        record = self.runtime.objc.registry.views.records[self.view]
        if not record.get('flipped', False):
            coordinates = list(coordinates)
            for i in range(1, len(coordinates), 2):
                coordinates[i] = record['frame'][3] - 1 - coordinates[i]
        getattr(self.runtime.objc.registry.views, 'probe_' + action)(self.view, *coordinates)
        after = self.values()
        result, payload = self.frame()
        result['parameters'] = [[i, b] for i, (a, b) in enumerate(zip(before, after)) if a != b]
        return result, payload

    def mouse(self, phase, coordinates):
        events = {'down': (1, 'mouseDown:'), 'drag': (6, 'mouseDragged:'), 'up': (2, 'mouseUp:')}
        if (phase not in events or not isinstance(coordinates, list) or len(coordinates) != 2
                or any(type(v) not in (int, float) or not math.isfinite(v) for v in coordinates)):
            raise CompatibilityError('Invalid streamed mouse event')
        if (phase == 'down') == self.mouse_pressed:
            raise CompatibilityError('Unbalanced streamed mouse event')
        record = self.runtime.objc.registry.views.records[self.view]
        x, y = coordinates
        if not record.get('flipped', False):
            y = record['frame'][3] - 1 - y
        before = self.values()
        kind, selector = events[phase]
        self.runtime.objc.registry.views.probe_mouse(self.view, [(kind, selector, x, y)])
        self.mouse_pressed = phase != 'up'
        after = self.values()
        result, payload = self.frame()
        result['parameters'] = [[i, b] for i, (a, b) in enumerate(zip(before, after)) if a != b]
        return result, payload

    def close(self):
        if self.session is None:
            return
        w = self.runtime
        try:
            if self.opened and not self.failed:
                w.dispatch(15)
                self.opened = False
        finally:
            try:
                if self.pool is not None:
                    w.objc.destroy(self.pool)
                    self.pool = None
            finally:
                w.close(suppress=True)
                self.session = None
