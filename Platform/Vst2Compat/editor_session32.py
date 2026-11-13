"""Isolated Carbon editor: offscreen frames and bounded complete gestures.

Legacy tracking yields outside CPU hooks to exchange frames and pointer state. Audio remains in its independent worker.
"""
import math
import struct
import threading

from carbon32 import fourcc
from editor_session import EditorSession
from libc_shims import release
from macho import CompatibilityError
from protocol import MAX_AUDIO
from session32 import Session32


class EditorSession32(EditorSession):
    def __init__(self, request):
        self.session = None
        self.opened = False
        self.failed = False
        self.tracking = None
        self.mouse_pressed = False
        self.pending_parameters = []
        try:
            self.session = Session32(request, editor_support=True)
            w = self.runtime
            if not w.flags & 1:
                raise CompatibilityError('Plugin does not advertise an original editor')
            if request.get('state') is not None:
                self.session.load_state(request['state'], include_parameters=False)
            self.apply_parameters(request.get('parameters', []))
            self.opened = bool(w.dispatch(14, pointer=w.carbon.window))
            if not self.opened:
                raise CompatibilityError('Plugin rejected Carbon editor open')
            output = w.allocate(4)
            try:
                w.cpu.mem_write(output, bytes(4))
                w.dispatch(13, pointer=output)
                rect = w.read_u32(output)
                if not rect:
                    raise CompatibilityError('Editor did not provide dimensions')
                top, left, bottom, right = struct.unpack('<4h', w.read(rect, 8))
            finally:
                release(w, output)
            self.width, self.height = right - left, bottom - top
            self.context = w.canvas.create(self.width, self.height)
            w.carbon.drawing_context = self.context
            candidates = [(token, record) for token, record in w.carbon.controls.items()
                          if 'class_token' in record and record['visible']]
            if len(candidates) != 1:
                raise CompatibilityError('Editor requires a single drawable Carbon control')
            self.control, record = candidates[0]
            self.klass = w.carbon.classes[record['class_token']]
        except Exception:
            self.failed = True
            self.close()
            raise

    def control_event(self, kind, parameters):
        w = self.runtime
        parameters = {fourcc('----'): (fourcc('ctrl'), struct.pack('<I', self.control)), **parameters}
        event = w.carbon.new_event(fourcc('cntl'), kind, parameters)
        try:
            result = w.call(self.klass['handler'], 0, event, self.klass['data'])
            if result not in (0, (-9874) & 0xffffffff):
                raise CompatibilityError(f'Carbon editor event failed: {result}')
            return result
        finally:
            w.carbon.events.pop(event, None)

    def frame(self):
        if (self.tracking is not None and not self.tracking.done and
                threading.current_thread() is not self.tracking.thread):
            raise CompatibilityError('Cannot redraw while Carbon mouse tracking is active')
        w = self.runtime
        w.dispatch(19)
        self.control_event(4, {
            fourcc('cprt'): (fourcc('cprt'), struct.pack('<h', 0)),
            fourcc('cntx'): (fourcc('cntx'), struct.pack('<I', self.context))})
        return self.bitmap()

    def bitmap(self):
        payload = self.runtime.canvas.png(self.context)
        if len(payload) > MAX_AUDIO:
            raise CompatibilityError('Editor PNG exceeds worker frame limit')
        return dict(encoding='png', width=self.width, height=self.height), payload

    def gesture(self, action, coordinates):
        if self.tracking is not None:
            raise CompatibilityError('Cannot overlap Carbon gestures')
        count = 2 if action == 'click' else 4 if action == 'drag' else 0
        if (not count or not isinstance(coordinates, list) or len(coordinates) != count or
                any(type(v) not in (int, float) or not math.isfinite(v) or
                    not -32768 <= v <= 32767 for v in coordinates)):
            raise CompatibilityError('Invalid Carbon editor gesture')
        x, y = map(int, coordinates[:2])
        if not 0 <= x < self.width or not 0 <= y < self.height:
            raise CompatibilityError('Carbon mouse down outside editor bounds')
        end_x, end_y = map(int, coordinates[-2:])
        samples = [(x, y, 1, 0)] * 2
        if action == 'drag':
            samples += [(round(x + (end_x-x)*i/8), round(y + (end_y-y)*i/8), 1, 0)
                        for i in range(1, 9)]
        iterator = iter(samples)
        w = self.runtime
        before = self.values()
        w.carbon.mouse_position = (x, y)
        w.carbon.input_source = lambda: next(iterator, (end_x, end_y, 0, 0))
        try:
            self.click_event(x, y)
        finally:
            w.carbon.input_source = None
            w.carbon.mouse_buttons = 0
        after = self.values()
        result, payload = self.frame()
        result['parameters'] = [[i, b] for i, (a, b) in enumerate(zip(before, after)) if a != b]
        return result, payload

    def click_event(self, x, y):
        return self.control_event(51, {
            fourcc('mloc'): (fourcc('hipt'), struct.pack('<2f', x, y)),
            fourcc('mbtn'): (fourcc('mbtn'), struct.pack('<H', 1)),
            fourcc('kmod'): (fourcc('magn'), struct.pack('<I', 0)),
            fourcc('ccnt'): (fourcc('magn'), struct.pack('<I', 1))})

    def apply_direct_parameters(self, parameters):
        super().apply_parameters(parameters)

    def apply_parameters(self, parameters):
        if self.tracking is None:
            return self.apply_direct_parameters(parameters)
        if not isinstance(parameters, list) or len(parameters) > self.runtime.params:
            raise CompatibilityError('Invalid editor parameter update list')
        seen = set()
        for item in parameters:
            if not isinstance(item, (list, tuple)) or len(item) != 2:
                raise CompatibilityError('Invalid editor parameter update')
            index, value = item
            if (type(index) is not int or index in seen or not 0 <= index < self.runtime.params or
                    type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1):
                raise CompatibilityError('Invalid editor parameter value')
            seen.add(index)
        self.pending_parameters = [list(item) for item in parameters]

    def mouse(self, phase, coordinates):
        from tracking32 import Tracking32
        # Input data is copied to the guest thread at its next safe yield.
        if (phase not in ('down', 'drag', 'up') or not isinstance(coordinates, list) or
                len(coordinates) != 2 or any(type(v) not in (int, float) or not math.isfinite(v) or
                not -32768 <= v <= 32767 for v in coordinates)):
            raise CompatibilityError('Invalid streamed Carbon mouse event')
        if (phase == 'down') == self.mouse_pressed:
            raise CompatibilityError('Unbalanced streamed Carbon mouse event')
        x, y = map(int, coordinates)
        if phase == 'down':
            if not 0 <= x < self.width or not 0 <= y < self.height:
                raise CompatibilityError('Carbon mouse down outside editor bounds')
            self.last_values = self.values()
            self.tracking = Tracking32(self, x, y)
            self.tracking.thread.start()
            snapshot = self.tracking.receive()
        else:
            snapshot = self.tracking.advance(x, y, phase != 'up', self.pending_parameters)
            if snapshot is None:
                # Some controls finish immediately on mouse-down.
                self.tracking.thread.join()
                self.apply_direct_parameters(self.pending_parameters)
                result, payload = self.frame()
                snapshot = result, payload, self.values()
        self.pending_parameters = []
        result, payload, values = snapshot
        result['parameters'] = [[i, b] for i, (a, b) in enumerate(zip(self.last_values, values)) if a != b]
        self.last_values = values
        self.mouse_pressed = phase != 'up'
        if phase == 'up':
            self.tracking.stop()
            self.tracking = None
        return result, payload

    def select_menu(self, selection):
        if self.tracking is None:
            raise CompatibilityError('No Carbon popup is active')
        result, payload, values = self.tracking.select_menu(selection)
        result['parameters'] = [[i, b] for i, (a, b) in enumerate(zip(self.last_values, values)) if a != b]
        self.last_values = values
        self.pending_parameters = []
        self.mouse_pressed = False
        if 'menu' not in result:
            self.tracking.stop()
            self.tracking = None
        return result, payload

    def close(self):
        if self.session is None:
            return
        w = self.runtime
        if self.tracking is not None:
            self.tracking.stop()
            self.tracking = None
        try:
            if self.opened and not self.failed:
                w.dispatch(15)
                self.opened = False
        finally:
            w.close(suppress=True)
            self.session = None
