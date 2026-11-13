"""Cooperative legacy mouse tracking; one thread exclusively owns guest execution.

Input polls yield through CallBatch outside Unicorn hooks. The protocol thread
only exchanges plain input/snapshot data; it never accesses the running CPU.
"""
import threading
import time
from darwin32 import CallBatch
from macho import CompatibilityError


class Tracking32:
    def __init__(self, editor, x, y):
        self.editor = editor
        self.values = editor.values()
        editor.runtime.editor_automation.clear()
        self.condition = threading.Condition()
        self.state = (x, y, 1, 0)
        self.pending = None
        self.menu_waiting = False
        self.menu_selection = None
        self.menu_allowed = set()
        self.reply = None
        self.done = False
        self.error = None
        self.sample_started = time.monotonic()
        self.sample_reads = editor.runtime.carbon.mouse_reads
        self.stopping = False
        self.thread = threading.Thread(target=self.run, daemon=True)

    def snapshot(self, final=False):
        editor = self.editor
        # During tracking the guest owns a live drawing context. Copy its
        # current pixels without re-entering the editor's draw callback.
        if final:
            details, payload = editor.frame()
        else:
            details, payload = editor.bitmap()
        if final:
            self.values = editor.values()
        else:
            for index, value in editor.runtime.editor_automation.items():
                self.values[index] = value
            editor.runtime.editor_automation.clear()
        return details, payload, list(self.values)

    def poll(self):
        return CallBatch([], self.yield_input)

    def yield_input(self, _):
        w = self.editor.runtime
        elapsed = time.monotonic() - self.sample_started
        sampled = w.carbon.mouse_reads >= self.sample_reads + 4
        if self.state[2] and ((sampled and elapsed >= 0.016) or elapsed >= 0.05):
            snapshot = self.snapshot()
            with self.condition:
                self.reply = snapshot
                self.condition.notify_all()
                ready = self.condition.wait_for(lambda: self.pending is not None or self.stopping, timeout=60)
                if not ready:
                    raise CompatibilityError('Carbon mouse tracking input timed out')
                w.resume_editor_input_budget()
                if self.stopping:
                    self.state = (*self.state[:2], 0, 0)
                    parameters = []
                else:
                    self.state, parameters = self.pending
                    self.pending = None
            for index, value in parameters:
                if self.values[index] != value:
                    w.set_parameter(index, value)
                    self.values[index] = value
            self.sample_started = time.monotonic()
            self.sample_reads = w.carbon.mouse_reads
        x, y, buttons, modifiers = self.state
        w.carbon.mouse_position = (x, y)
        w.carbon.mouse_buttons, w.carbon.key_modifiers = buttons, modifiers
        return buttons

    def popup(self, token, top, left, initial):
        w = self.editor.runtime
        items = w.menus.describe(token)
        allowed = set()
        def collect(entries, enabled=True):
            for item in entries:
                active = enabled and item['enabled']
                if 'children' in item:
                    collect(item['children'], active)
                elif active and not item['separator']:
                    allowed.add(item['selection'])
        collect(items)
        def wait_selection(_):
            details, payload, values = self.snapshot()
            details['menu'] = dict(items=items, x=self.state[0], y=self.state[1])
            with self.condition:
                self.menu_allowed = allowed
                self.menu_waiting = True
                self.reply = details, payload, values
                self.condition.notify_all()
                ready = self.condition.wait_for(lambda: self.menu_selection is not None or self.stopping, timeout=60)
                if not ready:
                    raise CompatibilityError('Carbon popup selection timed out')
                selection = 0 if self.stopping else self.menu_selection
                self.menu_selection = None
                self.menu_waiting = False
                self.state = (*self.state[:2], 0, 0)
                w.carbon.mouse_buttons = 0
                w.resume_editor_input_budget()
                return selection
        return CallBatch([], wait_selection)

    def select_menu(self, selection):
        with self.condition:
            if (not self.menu_waiting or type(selection) is not int or
                    (selection != 0 and selection not in self.menu_allowed)):
                raise CompatibilityError('Invalid Carbon popup selection')
            self.menu_selection = selection
            self.condition.notify_all()
        return self.receive()

    def run(self):
        w = self.editor.runtime
        try:
            w.carbon.mouse_position = self.state[:2]
            w.carbon.input_source = self.poll
            w.menus.popup = self.popup
            self.editor.click_event(*self.state[:2])
            w.carbon.input_source = None
            w.carbon.mouse_buttons = 0
            reply = self.snapshot(final=True)
            with self.condition:
                self.reply = reply
        except Exception as error:
            with self.condition:
                self.error = error
        finally:
            w.menus.popup = None
            w.carbon.input_source = None
            w.carbon.mouse_buttons = 0
            with self.condition:
                self.done = True
                self.condition.notify_all()

    def receive(self):
        with self.condition:
            if not self.condition.wait_for(lambda: self.reply is not None or self.done, timeout=3):
                raise CompatibilityError('Carbon tracking did not yield within its deadline')
            if self.error:
                raise self.error
            reply, self.reply = self.reply, None
            if reply is None:
                raise CompatibilityError('Carbon tracking returned no snapshot')
            return reply

    def advance(self, x, y, pressed, parameters):
        with self.condition:
            if self.done:
                return None
            if self.pending is not None:
                raise CompatibilityError('Overlapping Carbon input request')
            self.pending = ((x, y, int(pressed), 0), parameters)
            self.condition.notify_all()
        return self.receive()

    def stop(self):
        with self.condition:
            self.stopping = True
            self.condition.notify_all()
        self.thread.join(timeout=3)
        if self.thread.is_alive():
            raise CompatibilityError('Carbon tracking thread did not stop')
