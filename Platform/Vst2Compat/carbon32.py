"""Guest-only Carbon class registration; no native windows or callbacks.

ABI reference: Apple's Mac OS X 10.6 CarbonEvents.h,
RegisterToolboxObjectClass / UnregisterToolboxObjectClass.
"""
import struct
import math
import time
from macho import CompatibilityError
from darwin32 import CallBatch

SYMBOLS = ('_DisposeControl', '_SetThemeCursor', '_RunCurrentEventLoop', '_ReceiveNextEvent', '_IsWindowCollapsed', '_IsWindowVisible', '_SyncCGContextOriginWithPort', '_GetGrayRgn', '_GetRegionBounds', '_GetWindowBounds', '_GetWindowPort', '_GetPortBounds', '_QDSwapPort', '_QDBeginCGContext', '_QDEndCGContext',
           '_GetMouse', '_GetCurrentButtonState', '_GetCurrentEventButtonState', '_GetCurrentKeyModifiers',
           '_RegisterToolboxObjectClass', '_UnregisterToolboxObjectClass',
           '_NewEventHandlerUPP', '_DisposeEventHandlerUPP', '_CreateCustomControl',
           '_GetEventClass', '_GetEventKind', '_GetEventParameter', '_SetEventParameter',
           '_GetWindowEventTarget', '_GetControlEventTarget', '_InstallEventHandler', '_RemoveEventHandler',
           '_SetControlDragTrackingEnabled', '_SetAutomaticControlDragTrackingEnabledForWindow',
           '_GetWindowAttributes', '_HIViewGetRoot', '_HIViewFindByID', '_HIViewAddSubview',
           '_HIViewGetSuperview', '_HIViewRemoveFromSuperview', '_HIViewSetFrame',
           '_HIViewGetFrame', '_HIViewGetBounds', '_HIViewSetVisible', '_HIViewSetNeedsDisplay',
           '_NewDragReceiveHandlerUPP', '_NewDragTrackingHandlerUPP',
           '_DisposeDragReceiveHandlerUPP', '_DisposeDragTrackingHandlerUPP',
           '_InstallReceiveHandler', '_InstallTrackingHandler', '_RemoveReceiveHandler', '_RemoveTrackingHandler', '_Gestalt')


def fourcc(value):
    return int.from_bytes(value.encode('ascii'), 'big')


class Carbon32:
    def __init__(self, runtime):
        self.runtime = runtime
        self.classes = {}
        self.next_class = 0x75700000
        self.root, self.content = 0x75800000, 0x75800010
        self.controls = {self.root: dict(parent=0, frame=(0., 0., 1., 1.), visible=True),
                         self.content: dict(parent=self.root, frame=(0., 0., 1., 1.), visible=True)}
        self.events = {}
        self.next_control = 0x75800020
        self.next_event = 0x75900000
        self.window = 0x75600000  # offscreen host parent, never a native WindowRef
        self.targets = {}
        self.handlers = {}
        self.next_target = 0x75a00000
        self.next_handler = 0x75b00000
        self.window_drag_tracking = False
        self.drag_handlers = {}
        self.port = 0x75600010
        self.current_port = self.port
        self.drawing_context = None
        self.mouse_position = (0, 0)
        self.mouse_reads = 0
        self.mouse_buttons = 0
        self.key_modifiers = 0
        self.input_source = None
        self.cursor = 0

    def new_event(self, event_class, kind, parameters):
        if len(self.events) >= 256:
            raise CompatibilityError('i386 Carbon event limit exceeded')
        while True:
            if self.next_event >= 0x75a00000:
                self.next_event = 0x75900000
            token = self.next_event
            self.next_event += 16
            if token not in self.events:
                break
        self.events[token] = dict(event_class=event_class, kind=kind, parameters=parameters)
        return token

    def invoke(self, name, arg):
        w = self.runtime
        if name == '_DisposeControl':
            token = arg(0)
            if token not in self.controls or token in (self.root, self.content):
                raise CompatibilityError('Invalid i386 control disposal')
            ordered = []
            def collect(parent):
                for child, record in list(self.controls.items()):
                    if record['parent'] == parent:
                        collect(child)
                ordered.append(parent)
            collect(token)
            callbacks, events = [], []
            for control in ordered:
                record = self.controls[control]
                klass = self.classes.get(record.get('class_token'))
                if klass and (fourcc('cntl'), 1001) in klass['events']:
                    event = self.new_event(fourcc('cntl'), 1001, {
                        fourcc('----'): (fourcc('ctrl'), struct.pack('<I', control))})
                    events.append(event)
                    callbacks.append((klass['handler'], (0, event, klass['data'])))
            def complete(results):
                for event in events:
                    self.events.pop(event, None)
                targets = {target for target, owner in self.targets.items() if owner in ordered}
                self.handlers = {key: value for key, value in self.handlers.items() if value['target'] not in targets}
                for target in targets:
                    del self.targets[target]
                for control in ordered:
                    self.controls.pop(control, None)
                return 0
            return CallBatch(callbacks, complete)
        if name == '_SetThemeCursor':
            # Record a virtual cursor; never change the desktop pointer.
            if arg(0) > 127:
                return -50
            self.cursor = arg(0)
            return 0
        if name == '_RunCurrentEventLoop':
            timeout = struct.unpack('<d', struct.pack('<II', arg(0), arg(1)))[0]
            if not math.isfinite(timeout) or not 0 <= timeout <= 0.05:
                raise CompatibilityError('Unsupported i386 event-loop wait')
            time.sleep(timeout)
            return -9875
        if name == '_ReceiveNextEvent':
            count, types = arg(0), arg(1)
            if count > 256:
                raise CompatibilityError('i386 event filter limit exceeded')
            if count:
                w.read(types, count * 8)
            timeout = struct.unpack('<d', struct.pack('<II', arg(2), arg(3)))[0]
            if timeout != 0:
                raise CompatibilityError('Blocking i386 event receive not implemented')
            if arg(5):
                w.cpu.mem_write(arg(5), bytes(4))
            return -9875  # eventLoopTimedOutErr: virtual queue is empty.
        if name in ('_GetCurrentButtonState', '_GetCurrentEventButtonState'):
            if self.input_source is not None:
                state = self.input_source()
                if isinstance(state, CallBatch):
                    return state
                x, y, buttons, modifiers = state
                if (any(type(v) is not int for v in (x, y, buttons, modifiers)) or
                        not -32768 <= x <= 32767 or not -32768 <= y <= 32767 or
                        not 0 <= buttons <= 0xffffffff or not 0 <= modifiers <= 0xffffffff):
                    raise CompatibilityError('Invalid i386 host pointer state')
                self.mouse_position, self.mouse_buttons, self.key_modifiers = (x, y), buttons, modifiers
            return self.mouse_buttons
        if name == '_GetCurrentKeyModifiers':
            return self.key_modifiers
        if name == '_GetMouse':
            self.mouse_reads += 1
            x, y = self.mouse_position
            w.cpu.mem_write(arg(0), struct.pack('<hh', y, x))
            return 0
        if name == '_SyncCGContextOriginWithPort':
            if arg(0) != self.drawing_context or arg(1) != self.port:
                raise CompatibilityError('Unknown i386 context/port origin')
            return 0  # The virtual port has a fixed zero origin.
        if name in ('_IsWindowCollapsed', '_IsWindowVisible'):
            if arg(0) != self.window:
                raise CompatibilityError('Unknown i386 Carbon window')
            return int(name == '_IsWindowVisible')
        if name == '_GetGrayRgn':
            return 0x75600020
        if name == '_GetRegionBounds':
            if arg(0) != 0x75600020:
                raise CompatibilityError('Unknown virtual Carbon region')
            w.cpu.mem_write(arg(1), struct.pack('<4h', 0, 0, 4096, 4096))
            return arg(1)
        if name == '_GetWindowBounds':
            surface = w.canvas.surfaces.get(self.drawing_context)
            if arg(0) != self.window or arg(1) not in (32, 33) or surface is None:
                return -50
            w.cpu.mem_write(arg(2), struct.pack('<4h', 0, 0, surface['height'], surface['width']))
            return 0
        if name == '_GetWindowPort':
            if arg(0) != self.window:
                raise CompatibilityError('Unknown i386 Carbon window')
            return self.port
        if name == '_QDSwapPort':
            port = arg(0)
            if port != self.port:
                raise CompatibilityError('Unsupported i386 QuickDraw port')
            previous = self.current_port
            if arg(1):
                w.cpu.mem_write(arg(1), struct.pack('<I', previous))
            self.current_port = port
            return int(previous != port)
        if name in ('_GetPortBounds', '_QDBeginCGContext', '_QDEndCGContext'):
            surface = w.canvas.surfaces.get(self.drawing_context)
            if arg(0) != self.port or surface is None:
                raise CompatibilityError('No i386 offscreen drawing port')
            if name == '_GetPortBounds':
                w.cpu.mem_write(arg(1), struct.pack('<4h', 0, 0, surface['height'], surface['width']))
                return arg(1)
            if name == '_QDBeginCGContext':
                if surface.get('port_active'):
                    raise CompatibilityError('Nested i386 QuickDraw context acquisition')
                surface['port_active'] = True
                surface['port_depth'] = surface['depth']
                native, context = w.canvas.native, surface['context']
                native.CGContextSaveGState(context)
                native.CGContextScaleCTM(context, 1.0, -1.0)
                native.CGContextTranslateCTM(context, 0.0, -float(surface['height']))
                w.cpu.mem_write(arg(1), struct.pack('<I', self.drawing_context))
            elif w.read_u32(arg(1)) != self.drawing_context:
                raise CompatibilityError('Mismatched i386 QuickDraw context')
            else:
                if not surface.get('port_active'):
                    raise CompatibilityError('Unbalanced i386 QuickDraw context release')
                if surface['depth'] != surface['port_depth']:
                    raise CompatibilityError('Unbalanced i386 QuickDraw graphics state')
                w.canvas.native.CGContextRestoreGState(surface['context'])
                surface['port_active'] = False
                w.cpu.mem_write(arg(1), bytes(4))
            return 0
        if name == '_Gestalt':
            # Version of the legacy guest API profile, not the ARM host OS.
            versions = {fourcc('sysv'): 0x1060, fourcc('sys1'): 10, fourcc('sys2'): 6, fourcc('sys3'): 0}
            if arg(0) not in versions:
                return -5551  # gestaltUndefSelectorErr
            if not arg(1):
                return -50
            w.cpu.mem_write(arg(1), struct.pack('<I', versions[arg(0)]))
            return 0
        if name in ('_InstallReceiveHandler', '_InstallTrackingHandler', '_RemoveReceiveHandler', '_RemoveTrackingHandler'):
            callback, window = arg(0), arg(1)
            if not callback or window not in (0, self.window):
                return -50
            kind = 'receive' if 'Receive' in name else 'tracking'
            key = (kind, window, callback)
            if name.startswith('_Install'):
                if len(self.drag_handlers) >= 256:
                    raise CompatibilityError('i386 drag-handler limit exceeded')
                self.drag_handlers[key] = arg(2)
            elif key in self.drag_handlers:
                del self.drag_handlers[key]
            else:
                return -50
            return 0
        if name == '_HIViewGetRoot':
            if arg(0) != self.window:
                raise CompatibilityError('Unknown i386 Carbon window')
            return self.root
        if name == '_HIViewFindByID':
            start, signature, ident, output = (arg(i) for i in range(4))
            if start not in self.controls or not output:
                return -50
            found = self.content if start in (self.root, self.content) and (signature, ident) == (fourcc('wind'), 1) else 0
            w.cpu.mem_write(output, struct.pack('<I', found))
            return 0 if found else -30584  # errControlNotFound
        if name.startswith('_HIView'):
            token = arg(0)
            control = self.controls.get(token)
            if control is None:
                return -50
            if name == '_HIViewAddSubview':
                child = arg(1)
                if child not in self.controls or child in (self.root, self.content):
                    return -50
                ancestor = token
                while ancestor:
                    if ancestor == child:
                        return -50
                    ancestor = self.controls[ancestor]['parent']
                self.controls[child]['parent'] = token
            elif name == '_HIViewRemoveFromSuperview':
                if token in (self.root, self.content):
                    return -50
                control['parent'] = 0
            elif name == '_HIViewGetSuperview':
                return control['parent']
            elif name == '_HIViewSetFrame':
                frame = struct.unpack('<4f', w.read(arg(1), 16))
                if any(not math.isfinite(v) or abs(v) > 65536 for v in frame) or min(frame[2:]) < 0:
                    return -50
                control['frame'] = frame
            elif name in ('_HIViewGetFrame', '_HIViewGetBounds'):
                frame = control['frame']
                if name == '_HIViewGetBounds':
                    frame = (0., 0., frame[2], frame[3])
                w.cpu.mem_write(arg(1), struct.pack('<4f', *frame))
            elif name == '_HIViewSetVisible':
                control['visible'] = bool(arg(1) & 255)
            elif name == '_HIViewSetNeedsDisplay':
                control['needs_display'] = bool(arg(1) & 255)
            else:
                raise CompatibilityError('Unimplemented i386 HIView operation')
            return 0
        if name == '_GetWindowAttributes':
            if arg(0) != self.window or not arg(1):
                return -50
            # Offscreen parent will use composited HIView drawing events.
            w.cpu.mem_write(arg(1), struct.pack('<I', 1 << 19))
            return 0
        if name == '_SetControlDragTrackingEnabled':
            if arg(0) not in self.controls:
                return -50
            self.controls[arg(0)]['drag_tracking'] = bool(arg(1) & 255)
            return 0
        if name == '_SetAutomaticControlDragTrackingEnabledForWindow':
            if arg(0) != self.window:
                return -50
            self.window_drag_tracking = bool(arg(1) & 255)
            return 0
        if name in ('_GetWindowEventTarget', '_GetControlEventTarget'):
            owner = arg(0)
            if ((name == '_GetWindowEventTarget' and owner != self.window) or
                    (name == '_GetControlEventTarget' and owner not in self.controls)):
                raise CompatibilityError('Unknown i386 Carbon event target owner')
            for token, stored in self.targets.items():
                if stored == owner:
                    return token
            token = self.next_target
            if token >= 0x75b00000:
                raise CompatibilityError('i386 Carbon event target limit exceeded')
            self.next_target += 16
            self.targets[token] = owner
            return token
        if name == '_InstallEventHandler':
            target, callback, count, events, data, output = (arg(i) for i in range(6))
            if target not in self.targets or not callback:
                return -50
            if count > 256 or len(self.handlers) >= 256 or self.next_handler >= 0x75c00000:
                raise CompatibilityError('i386 Carbon handler limit exceeded')
            event_types = list(struct.iter_unpack('<II', w.read(events, count * 8))) if count else []
            token = self.next_handler
            self.next_handler += 16
            self.handlers[token] = dict(target=target, callback=callback, events=event_types, data=data)
            if output:
                w.cpu.mem_write(output, struct.pack('<I', token))
            return 0
        if name == '_RemoveEventHandler':
            if arg(0) not in self.handlers:
                return -50
            del self.handlers[arg(0)]
            return 0
        if name in ('_GetEventClass', '_GetEventKind', '_GetEventParameter', '_SetEventParameter'):
            event = self.events.get(arg(0))
            if event is None:
                raise CompatibilityError('Unknown i386 Carbon event')
            if name == '_GetEventClass':
                return event['event_class']
            if name == '_GetEventKind':
                return event['kind']
            key, typ = arg(1), arg(2)
            if name == '_SetEventParameter':
                size = arg(3)
                if size > 65536 or (key not in event['parameters'] and len(event['parameters']) >= 256):
                    raise CompatibilityError('i386 Carbon event parameter limit exceeded')
                event['parameters'][key] = (typ, w.read(arg(4), size))
                return 0
            if key not in event['parameters']:
                return -9870  # eventParameterNotFoundErr
            actual, data = event['parameters'][key]
            if typ not in (0, fourcc('****'), actual):
                raise CompatibilityError('Unsupported i386 Carbon event parameter coercion')
            if arg(3):
                w.cpu.mem_write(arg(3), struct.pack('<I', actual))
            if arg(5):
                w.cpu.mem_write(arg(5), struct.pack('<I', len(data)))
            if arg(6):
                if arg(4) < len(data):
                    return -50
                w.cpu.mem_write(arg(6), data)
            return 0
        if name == '_CreateCustomControl':
            owner, bounds, definition, collection, output = (arg(i) for i in range(5))
            if owner:
                raise CompatibilityError('i386 control owning window not implemented')
            if not output:
                return -50
            kind, class_token = struct.unpack('<II', w.read(definition, 8))
            if kind != 1 or class_token not in self.classes:
                raise CompatibilityError('Unsupported i386 control definition')
            if len(self.controls) >= 256 or self.next_control >= 0x75900000:
                raise CompatibilityError('i386 Carbon control limit exceeded')
            rect = struct.unpack('<4h', w.read(bounds, 8))
            token = self.next_control
            self.next_control += 16
            record = self.classes[class_token]
            top, left, bottom, right = rect
            self.controls[token] = dict(class_token=class_token, bounds=rect, features=0,
                                       frame=(float(left), float(top), float(right-left), float(bottom-top)),
                                       parent=0, visible=True)
            w.cpu.mem_write(output, bytes(4))
            event = self.new_event(fourcc('cntl'), 1000, {
                fourcc('----'): (fourcc('ctrl'), struct.pack('<I', token)),
                fourcc('icol'): (fourcc('cltn'), struct.pack('<I', collection))})
            def complete(results):
                status = results[0] if results else 0
                parameters = self.events.pop(event)['parameters']
                if status not in (0, (-9874) & 0xffffffff):
                    del self.controls[token]
                    return status
                features = parameters.get(fourcc('cftr'))
                if features:
                    if features[0] != fourcc('magn') or len(features[1]) != 4:
                        raise CompatibilityError('Invalid i386 control features')
                    self.controls[token]['features'] = struct.unpack('<I', features[1])[0]
                w.cpu.mem_write(output, struct.pack('<I', token))
                return 0
            callbacks = [(record['handler'], (0, event, record['data']))] if (fourcc('cntl'), 1000) in record['events'] else []
            return CallBatch(callbacks, complete)
        if name in ('_NewEventHandlerUPP', '_DisposeEventHandlerUPP',
                    '_NewDragReceiveHandlerUPP', '_NewDragTrackingHandlerUPP',
                    '_DisposeDragReceiveHandlerUPP', '_DisposeDragTrackingHandlerUPP'):
            # Mach-O Carbon UPPs are plain guest function pointers. The host
            # must only invoke these through the emulator's callback machinery.
            if not arg(0):
                raise CompatibilityError('Null i386 event callback')
            return arg(0) if name.startswith('_New') else 0
        if name == '_UnregisterToolboxObjectClass':
            if arg(0) not in self.classes:
                return -50
            del self.classes[arg(0)]
            return 0
        identifier = w.cf.constant_string(arg(0))
        base, count, events, handler, data, output = (arg(i) for i in range(1, 7))
        if base and base not in self.classes:
            raise CompatibilityError('Unknown i386 Carbon base class')
        if count > 256 or len(self.classes) >= 256 or self.next_class >= 0x75800000:
            raise CompatibilityError('i386 Carbon class/event limit exceeded')
        if not handler or not output:
            return -50
        if any(record['identifier'] == identifier for record in self.classes.values()):
            raise CompatibilityError('Duplicate i386 Carbon class registration')
        event_types = list(struct.iter_unpack('<II', w.read(events, count * 8))) if count else []
        token = self.next_class
        self.next_class += 16
        w.cpu.mem_write(output, struct.pack('<I', token))
        self.classes[token] = dict(identifier=identifier, base=base, events=event_types,
                                   handler=handler, data=data)
        return 0
