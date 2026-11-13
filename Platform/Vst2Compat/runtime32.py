"""Isolated, bounded i386 execution foundation for offline compatibility work.

This is not connected to the audio host yet. Unknown Mac functions and data
remain unmapped guards: the first access stops execution with a symbol name.
"""
import platform
import struct
import time
import math
import graphics32
import cf32
import darwin32
import libc_shims
import format_shims
from format32 import Arguments32
import stdio32
import filesystem_shims
import wide_shims
import math32
import random32
import resource32
import carbon32
import menu32
import image32
import quickdraw32
import canvas32
from resource_paths import ResourcePaths

from macho import CompatibilityError, MAX_IMAGE

PAGE = 4096
STACK, STACK_SIZE = 0x70000000, 1024 * 1024
HEAP, HEAP_SIZE = 0x71000000, 32 * 1024 * 1024
GATES, GATE_SIZE = 0x74000000, 4 * PAGE
RETURN = GATES + GATE_SIZE - 1
HOST = GATES + GATE_SIZE - 0x200
FLOAT_RETURN = HOST + 0x40
DATA = GATES + GATE_SIZE
MISSING = 0x78000000
FILESYSTEM_SYMBOLS = ('_FSFindFolder', '_FSRefMakePath', '_FSPathMakeRef')


class Runtime32:
    SHIMS = tuple(dict.fromkeys(('_malloc', '_calloc', '_free', '_memcpy', '_memmove', '_memset', '_strlen',
             '_bzero', '___bzero', '__Znwm', '__Znam', '__ZdlPv', '__ZdaPv',
             '_CMOpenProfile') + graphics32.SYMBOLS + cf32.SYMBOLS + darwin32.SYMBOLS + libc_shims.SYMBOLS + format_shims.SYMBOLS + stdio32.SYMBOLS + FILESYSTEM_SYMBOLS + wide_shims.SYMBOLS + math32.SYMBOLS + random32.SYMBOLS + resource32.SYMBOLS + carbon32.SYMBOLS + menu32.SYMBOLS + image32.SYMBOLS + quickdraw32.SYMBOLS + canvas32.SYMBOLS))

    def __init__(self, image, *, resource_root=None, startup_seconds=15, editor_support=False):
        self.editor_support = editor_support
        self.input_epoch = 0
        self.editor_automation = {}
        if getattr(image, 'architecture', None) != 'i386':
            raise CompatibilityError('The i386 runtime requires an i386 image')
        if platform.machine().lower() not in ('arm64', 'aarch64'):
            raise CompatibilityError('Use native ARM64 Python; Rosetta is not used')
        if image.unsupported:
            raise CompatibilityError('Unsupported i386 loader features: ' + ', '.join(sorted(set(image.unsupported))))
        if not math.isfinite(startup_seconds) or not 0.1 <= startup_seconds <= 30:
            raise CompatibilityError('i386 startup budget must be 0.1..30 seconds')
        self.startup_seconds = startup_seconds
        import unicorn as uc
        from unicorn import x86_const as x86
        self.uc, self.x86, self.image = uc, x86, image
        self.resource_paths = ResourcePaths(resource_root)
        self.cpu = uc.Uc(uc.UC_ARCH_X86, uc.UC_MODE_32)
        self.phase, self.completed_initializers = 'mapping', 0
        self.error = None
        self.failed = False
        self.closed = False
        self.call_depth = 0
        self.deadlines = []
        self.pending_calls = None
        self.effect = 0
        self.opened = False
        self.active = self.started = False
        self.position = 0
        self.audio_inputs, self.audio_outputs = [], []
        self.sample_rate, self.block_size = 48000, 256
        self.host_calls = {}
        self.parameter_buffer = None
        self.unknown_host_calls = set()
        self.heap_next = HEAP
        self.allocations = {}
        self.free_blocks = []
        self.time_info = 0  # reserved for future audio time-info ownership
        self.import_counts = {}
        self.graphics = graphics32.Graphics32(self)
        self.cf = cf32.CF32(self)
        self.carbon = carbon32.Carbon32(self)
        self.menus = menu32.Menu32(self)
        self.random = random32.Random32(self)
        self.images = image32.Images32(self)
        self.quickdraw = quickdraw32.QuickDraw32(self)
        self.canvas = canvas32.Canvas32(self)
        self.darwin = darwin32.Darwin32(self)
        self.filesystem = filesystem_shims.Filesystem(self)
        self.resources = resource32.Resources32(self)
        self.missing_import = None
        self.shims = {name: GATES + index * 16 for index, name in enumerate(self.SHIMS)}
        self.gate_names = {address: name for name, address in self.shims.items()}
        self.gate_names[HOST] = '<audioMaster>'
        if len(self.SHIMS) * 16 > HOST - GATES:
            raise CompatibilityError('i386 import gates exceed reserved space')
        self.data_exports = {'___sF': DATA + 256, '___stdinp': DATA,
                             '___stdoutp': DATA + 4, '___stderrp': DATA + 8,
                             '_kHIViewWindowContentID': DATA + 2400}
        bindings = list(image.bindings())
        names = set(image.imports) | {name for _, name, _ in bindings + image.stub_bindings}
        missing = sorted(names - image.symbols.keys() - self.shims.keys() - self.data_exports.keys())
        if len(missing) > 4096:
            raise CompatibilityError('Too many i386 unresolved imports')
        self.missing = {name: MISSING + index * PAGE + PAGE // 2 for index, name in enumerate(missing)}
        self.missing_pages = {address // PAGE: name for name, address in self.missing.items()}
        mapped = 0
        for segment in image.segments:
            if not segment.size:
                continue
            size = (segment.size + PAGE - 1) & -PAGE
            if segment.address % PAGE or segment.address + size > STACK:
                raise CompatibilityError('i386 image overlaps reserved memory or is not page aligned')
            mapped += size
            if mapped > MAX_IMAGE:
                raise CompatibilityError('i386 mapped image exceeds memory limit')
            self.cpu.mem_map(segment.address, size, segment.protection)
            if segment.file_size:
                self.cpu.mem_write(segment.address, image.data[segment.offset:segment.offset + segment.file_size])
        self.cpu.mem_map(STACK, STACK_SIZE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.cpu.mem_map(HEAP, HEAP_SIZE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.cpu.mem_map(GATES, GATE_SIZE, uc.UC_PROT_READ | uc.UC_PROT_EXEC)
        self.cpu.mem_write(GATES, b'\xc3' * GATE_SIZE)
        self.cpu.mem_map(DATA, PAGE, uc.UC_PROT_READ | uc.UC_PROT_WRITE)
        self.stdio = stdio32.Stdio32(self, DATA)
        self.time_info = DATA + 2304
        self.cpu.mem_write(DATA + 2400, struct.pack('<II', 0x77696e64, 1))
        self.math = math32.Math32(self, DATA + 1024)
        self.gate_tails = set()
        for name in math32.SYMBOLS:
            address = self.shims[name]
            self.cpu.mem_write(address, b'\xdd\x05' + struct.pack('<I', DATA + 1024) + b'\xc3')
            self.gate_tails.add(address + 6)
        # Consume an i386 float return through the guest x87 stack. FSTP m32fp
        # rounds exactly as a float-returning caller would, then reaches RETURN.
        self.cpu.mem_write(FLOAT_RETURN, b'\xd9\x1d' + struct.pack('<I', DATA + 2048) +
                           b'\xe9' + struct.pack('<I', (RETURN - FLOAT_RETURN - 11) & 0xffffffff))
        self.gate_tails.update((FLOAT_RETURN, FLOAT_RETURN + 6))

        def resolve(name):
            if name in image.symbols:
                return image.symbols[name]
            if name in self.data_exports:
                return self.data_exports[name]
            return self.shims[name] if name in self.shims else self.missing[name]

        for address, name, addend in bindings:
            self.cpu.mem_write(address, struct.pack('<I', (resolve(name) + addend) & 0xffffffff))
        for address, name, _ in image.stub_bindings:
            self.cpu.mem_write(address, b'\xe9' + struct.pack('<I', (resolve(name) - address - 5) & 0xffffffff))
        self.cpu.hook_add(uc.UC_HOOK_CODE, self.on_gate, begin=GATES, end=GATES + GATE_SIZE - 1)
        self.cpu.hook_add(uc.UC_HOOK_MEM_INVALID, self.on_invalid)

    def on_invalid(self, cpu, access, address, size, value, user):
        name = self.missing_pages.get(address // PAGE)
        self.missing_import = name
        self.error = CompatibilityError(f'Unimplemented i386 import {name}' if name else
                                        f'Invalid i386 guest memory access at 0x{address:x}')
        return False

    def read(self, address, size):
        if not 0 <= size <= HEAP_SIZE or address < 0 or address + size > 1 << 32:
            raise CompatibilityError('Invalid i386 guest memory range')
        return bytes(self.cpu.mem_read(address, size))

    def read_u32(self, address):
        return struct.unpack('<I', self.read(address, 4))[0]

    def arg(self, index):
        return self.read_u32(self.cpu.reg_read(self.x86.UC_X86_REG_ESP) + 4 + index * 4)

    def allocate(self, size):
        if not 0 <= size <= HEAP_SIZE:
            raise CompatibilityError('Invalid i386 allocation size')
        size = max(16, (size + 15) & -16)
        for index, (address, available) in enumerate(self.free_blocks):
            if available >= size:
                self.free_blocks.pop(index)
                if available > size:
                    self.free_blocks.append((address + size, available - size))
                self.allocations[address] = size
                return address
        address = self.heap_next
        if address + size > HEAP + HEAP_SIZE:
            raise CompatibilityError('i386 diagnostic heap exhausted')
        self.heap_next += size
        self.allocations[address] = size
        return address

    def on_gate(self, cpu, address, size, user):
        try:
            if address in self.gate_tails:
                return
            name = self.gate_names.get(address)
            if name is None:
                raise CompatibilityError('Invalid i386 import gate')
            self.import_counts[name] = self.import_counts.get(name, 0) + 1
            stack = cpu.reg_read(self.x86.UC_X86_REG_ESP)
            arg = lambda index: self.read_u32(stack + 4 + index * 4)
            if name == '<audioMaster>':
                opcode = arg(1)
                self.host_calls[opcode] = self.host_calls.get(opcode, 0) + 1
                if opcode == 0 and self.editor_support:
                    index = arg(2)
                    value = struct.unpack('<f', struct.pack('<I', arg(5)))[0]
                    if index >= self.params or not math.isfinite(value) or not 0 <= value <= 1:
                        raise CompatibilityError('Invalid editor automation value')
                    self.editor_automation[index] = value
                    result = 0
                elif opcode == 1:
                    result = 2400
                elif opcode == 16:
                    result = int(self.sample_rate)
                elif opcode == 17:
                    result = self.block_size
                elif opcode == 7:
                    flags = arg(3) & (0x200 | 0x400 | 0x2000)
                    if self.phase == 'VST process':
                        flags |= 2  # transport playing during offline rendering
                    cpu.mem_write(self.time_info, struct.pack('<8d6i', self.position, self.sample_rate, 0,
                                  self.position / self.sample_rate * 2, 120, 0, 0, 0, 4, 4, 0, 0, 0, flags))
                    result = self.time_info
                elif opcode == 23:
                    result = 4  # offline processing
                elif opcode in (32, 33):
                    cpu.mem_write(arg(4), b'Cardinal\0')
                    result = 1
                elif opcode == 34:
                    result = 1
                elif opcode in (2, 3, 15, 18, 19, 37):
                    result = 0  # unavailable/unknown; no audio is running yet
                elif opcode == 38:
                    result = 1  # English
                else:
                    self.unknown_host_calls.add(opcode)
                    result = 0
            elif name in resource32.SYMBOLS:
                result = self.resources.invoke(name, arg)
            elif name in random32.SYMBOLS:
                result = self.random.invoke(name, arg)
            elif name in math32.SYMBOLS:
                result = self.math.invoke(name)
            elif name in wide_shims.SYMBOLS:
                result = wide_shims.invoke(self, name)
            elif name in FILESYSTEM_SYMBOLS:
                result = self.filesystem.invoke(name)
            elif name in stdio32.SYMBOLS:
                result = self.stdio.invoke(name)
            elif name in format_shims.SYMBOLS:
                result = format_shims.invoke(self, name, Arguments32)
            elif name in darwin32.SYMBOLS:
                result = self.darwin.invoke(name, arg)
            elif name in canvas32.SYMBOLS:
                result = self.canvas.invoke(name, arg)
            elif name in quickdraw32.SYMBOLS:
                result = self.quickdraw.invoke(name, arg)
            elif name in image32.SYMBOLS:
                result = self.images.invoke(name, arg)
            elif name in menu32.SYMBOLS:
                result = self.menus.invoke(name, arg)
            elif name in carbon32.SYMBOLS:
                result = self.carbon.invoke(name, arg)
            elif name in cf32.SYMBOLS:
                result = self.cf.invoke(name, arg)
            elif name in graphics32.SYMBOLS:
                result = self.graphics.invoke(name, arg)
            elif name == '_CMOpenProfile':
                # No ColorSync profile service in this offline worker. Report
                # cmProfileNotFound, never a fake successful native reference.
                cpu.mem_write(arg(0), bytes(4))
                result = -176
            elif name in ('_malloc', '__Znwm', '__Znam'):
                result = self.allocate(arg(0))
            elif name == '_calloc':
                count = arg(0) * arg(1)
                result = self.allocate(count)
                cpu.mem_write(result, bytes(count))
            elif name in ('_free', '__ZdlPv', '__ZdaPv'):
                libc_shims.release(self, arg(0))
                result = 0
            elif name == '_strlen':
                result = len(libc_shims.string_bytes(self, arg(0)))
            elif name in libc_shims.SYMBOLS:
                result = libc_shims.invoke(self, name)
            elif name in ('_memcpy', '_memmove'):
                result = arg(0)
                cpu.mem_write(result, self.read(arg(1), arg(2)))
            else:
                result = arg(0)
                zero = name in ('_bzero', '___bzero')
                count, value = (arg(1), 0) if zero else (arg(2), arg(1) & 255)
                if count > HEAP_SIZE:
                    raise CompatibilityError('Oversized i386 memory fill')
                cpu.mem_write(result, bytes([value]) * count)
            if isinstance(result, darwin32.CallBatch):
                self.pending_calls = result
                cpu.reg_write(self.x86.UC_X86_REG_EAX, 0)
                cpu.emu_stop()
            elif result is not None:
                cpu.reg_write(self.x86.UC_X86_REG_EAX, result & 0xffffffff)
        except Exception as exc:
            self.error = exc
            cpu.emu_stop()

    def resume_editor_input_budget(self):
        if not self.editor_support:
            raise CompatibilityError('Input budget renewal is editor-only')
        # Only the host's receipt of a fresh input sample renews this budget.
        # Guest polling alone cannot grant itself more execution time.
        self.deadlines[:] = [time.monotonic() + 2 for _ in self.deadlines]
        self.input_epoch += 1

    def call(self, address, *arguments, floating=False):
        if self.closed:
            raise CompatibilityError('i386 runtime is closed')
        if self.failed:
            raise CompatibilityError('i386 runtime already faulted')
        if len(arguments) > 16:
            raise CompatibilityError('Too many i386 diagnostic call arguments')
        if self.call_depth >= 8:
            raise CompatibilityError('i386 callback nesting limit exceeded')
        # Darwin i386 cdecl: stack aligned to 16 immediately before CALL.
        top = (self.cpu.reg_read(self.x86.UC_X86_REG_ESP) - 256 if self.call_depth
               else STACK + STACK_SIZE - 256)
        stack = ((top - 4 * len(arguments)) & -16) - 4
        if stack < STACK + 4096:
            raise CompatibilityError('i386 callback stack exhausted')
        self.cpu.mem_write(stack, struct.pack('<' + 'I' * (1 + len(arguments)),
                                            FLOAT_RETURN if floating else RETURN, *arguments))
        self.cpu.reg_write(self.x86.UC_X86_REG_ESP, stack)
        self.error = None
        started = time.monotonic()
        startup = self.phase.startswith('initializer ') or self.phase in ('VST entry', 'VST open')
        seconds = self.startup_seconds if startup else 2
        instructions = max(1, int(seconds * 2_000_000)) if startup else 2_000_000
        # As in the 64-bit runtime, avoid per-instruction instrumentation in
        # DSP. The wall-clock deadline still stops runaway guest execution.
        if self.phase == 'VST process':
            instructions = 0
        deadline = min(started + seconds, *self.deadlines) if self.deadlines else started + seconds
        self.deadlines.append(deadline)
        self.call_depth += 1
        continuations = 0
        input_epoch = self.input_epoch
        try:
            while True:
                if input_epoch != self.input_epoch:
                    input_epoch = self.input_epoch
                    continuations = 0
                deadline = min(self.deadlines)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise CompatibilityError('i386 call exceeded its time/instruction budget')
                self.cpu.emu_start(address, RETURN, timeout=max(1, int(remaining * 1_000_000)), count=instructions)
                if self.error:
                    raise self.error
                if self.pending_calls is not None:
                    continuations += 1
                    if continuations > 4096:
                        raise CompatibilityError('i386 call exceeded its callback budget')
                    batch, self.pending_calls = self.pending_calls, None
                    context = self.cpu.context_save()
                    result = 0
                    try:
                        returns = [self.call(function, *args) for function, args in batch.callbacks]
                        if batch.completion:
                            result = batch.completion(returns)
                    finally:
                        self.cpu.context_restore(context)
                    self.cpu.reg_write(self.x86.UC_X86_REG_EAX, result & 0xffffffff)
                    # Complete the suspended gate's RET without dispatching
                    # that gate twice. Nested emulation happens outside hooks.
                    stack = self.cpu.reg_read(self.x86.UC_X86_REG_ESP)
                    address = self.read_u32(stack)
                    self.cpu.reg_write(self.x86.UC_X86_REG_ESP, stack + 4)
                    self.cpu.reg_write(self.x86.UC_X86_REG_EIP, address)
                    if address == RETURN:
                        break
                    continue
                if self.cpu.reg_read(self.x86.UC_X86_REG_EIP) != RETURN:
                    raise CompatibilityError('i386 call exceeded its time/instruction budget')
                break
            return (struct.unpack('<f', self.read(DATA + 2048, 4))[0] if floating else
                    self.cpu.reg_read(self.x86.UC_X86_REG_EAX))
        except Exception as exc:
            self.failed = True
            error = CompatibilityError(str(self.error or exc))
            error.details = self.diagnostics()
            error.details['call_seconds'] = time.monotonic() - started
            raise error from exc
        finally:
            self.call_depth -= 1
            self.deadlines.pop()

    def initialize(self):
        for index, slot in enumerate(self.image.initializers):
            self.phase = f'initializer {index + 1}/{len(self.image.initializers)}'
            self.call(self.read_u32(slot))
            self.completed_initializers += 1
        self.phase = 'initializers completed'

    def open_plugin(self):
        if self.effect or self.opened:
            raise CompatibilityError('i386 plugin entry already invoked')
        if self.image.entry is None:
            raise CompatibilityError('i386 VST entry point not found')
        self.phase = 'VST entry'
        self.effect = self.call(self.image.entry, HOST)
        if not self.effect:
            raise CompatibilityError('i386 VST entry returned no effect')
        words = struct.unpack('<21I', self.read(self.effect, 84))
        if words[0] != 0x56737450:
            raise CompatibilityError('Invalid i386 AEffect magic')
        self.dispatcher, self.processor, self.setter, self.getter = words[1:5]
        self.programs, self.params, self.inputs, self.outputs, self.flags = words[5:10]
        if self.programs > 4096 or self.params > 4096 or self.inputs > 64 or not 1 <= self.outputs <= 64:
            raise CompatibilityError('i386 AEffect exceeds supported dimensions')
        if not self.dispatcher or (self.params and (not self.setter or not self.getter)):
            raise CompatibilityError('i386 AEffect has missing callbacks')
        self.unique_id, self.version, self.replacing = words[18:21]
        self.phase = 'VST open'
        # Track partial open so a successfully returned open is closed later;
        # trapped guest execution is never resumed merely to run cleanup.
        self.opened = True
        self.dispatch(0)
        self.phase = 'VST opened'
        return dict(parameters=self.params, programs=self.programs, inputs=self.inputs,
                    outputs=self.outputs, unique_id=self.unique_id, version=self.version)

    def dispatch(self, opcode, index=0, value=0, pointer=0, option=0.0):
        bits, = struct.unpack('<I', struct.pack('<f', option))
        result = self.call(self.dispatcher, self.effect, opcode & 0xffffffff,
                           index & 0xffffffff, value & 0xffffffff, pointer, bits)
        return result if result < 0x80000000 else result - (1 << 32)

    def get_parameter(self, index):
        if not self.opened or type(index) is not int or not 0 <= index < self.params:
            raise CompatibilityError('Invalid i386 parameter index or plugin not open')
        value = self.call(self.getter, self.effect, index, floating=True)
        if not math.isfinite(value) or not 0 <= value <= 1:
            raise CompatibilityError('i386 plugin returned an invalid normalized parameter')
        return value

    def set_parameter(self, index, value):
        if (not self.opened or type(index) is not int or not 0 <= index < self.params or
                type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= 1):
            raise CompatibilityError('Invalid i386 normalized parameter assignment')
        bits, = struct.unpack('<I', struct.pack('<f', value))
        self.call(self.setter, self.effect, index, bits)

    def parameter_info(self, indices=None):
        self.phase = 'parameter metadata'
        if self.parameter_buffer is None:
            self.parameter_buffer = self.allocate(256)
        output = self.parameter_buffer
        result = []
        for index in range(self.params) if indices is None else indices:
            item = dict(id=index, value=self.get_parameter(index))
            for opcode, field in ((8, 'name'), (6, 'units'), (7, 'display')):
                self.cpu.mem_write(output, bytes(256))
                self.dispatch(opcode, index=index, pointer=output)
                item[field] = libc_shims.string_bytes(self, output, limit=256).decode('utf-8', errors='replace')
            result.append(item)
        return result

    def start_audio(self, sample_rate=48000, block_size=256):
        if not self.opened or self.active or self.audio_inputs or self.audio_outputs:
            raise CompatibilityError('Invalid i386 audio initialization state')
        if (type(sample_rate) not in (int, float) or not math.isfinite(sample_rate) or
                not 8000 <= sample_rate <= 384000 or type(block_size) is not int or not 1 <= block_size <= 8192):
            raise CompatibilityError('Invalid i386 sample rate or block size')
        processor = self.replacing if self.flags & 16 and self.replacing else self.processor
        if not processor:
            raise CompatibilityError('i386 plugin has no float audio processor')
        self.audio_processor = processor
        self.sample_rate, self.block_size = sample_rate, block_size
        self.audio_inputs = [self.allocate(block_size * 4) for _ in range(self.inputs)]
        self.audio_outputs = [self.allocate(block_size * 4) for _ in range(self.outputs)]
        self.input_table = self.allocate(max(4, self.inputs * 4))
        self.output_table = self.allocate(self.outputs * 4)
        self.midi_buffer = self.allocate(8 + 256 * (4 + 32))
        if self.inputs:
            self.cpu.mem_write(self.input_table, struct.pack('<' + 'I' * self.inputs, *self.audio_inputs))
        self.cpu.mem_write(self.output_table, struct.pack('<' + 'I' * self.outputs, *self.audio_outputs))
        self.phase = 'audio configuration'
        self.dispatch(10, option=float(sample_rate))
        self.dispatch(11, value=block_size)
        self.active = True
        self.dispatch(12, value=1)
        self.started = True
        self.dispatch(71)
        self.phase = 'audio ready'

    def process(self, inputs, frames, midi=()):
        if not self.active or type(frames) is not int or not 1 <= frames <= self.block_size:
            raise CompatibilityError('Invalid i386 audio frame count or inactive processor')
        if len(inputs) != self.inputs or any(len(channel) != frames for channel in inputs):
            raise CompatibilityError('i386 audio channel/frame shape mismatch')
        if any(type(value) not in (int, float) or not math.isfinite(value) for channel in inputs for value in channel):
            raise CompatibilityError('Non-finite or invalid i386 input audio')
        try:
            payloads = [struct.pack('<' + 'f' * frames, *channel) for channel in inputs]
        except (OverflowError, struct.error) as exc:
            raise CompatibilityError('i386 audio input exceeds float range') from exc
        if not isinstance(midi, (list, tuple)) or len(midi) > 256:
            raise CompatibilityError('Invalid i386 MIDI event list')
        for event in midi:
            if (not isinstance(event, (list, tuple)) or len(event) != 4 or
                    any(type(value) is not int for value in event) or
                    not 0 <= event[0] < frames or not 0x80 <= event[1] <= 0xef or
                    not 0 <= event[2] < 128 or not 0 <= event[3] < 128):
                raise CompatibilityError('Invalid i386 MIDI channel event')
        self.phase = 'VST process'
        if midi:
            events = self.midi_buffer + 8 + 4 * len(midi)
            header = struct.pack('<II', len(midi), 0)
            header += struct.pack('<' + 'I' * len(midi), *(events + i * 32 for i in range(len(midi))))
            payload = b''.join(struct.pack('<6i8B', 1, 32, delta, 0, 0, 0,
                                          status, data1, data2, 0, 0, 0, 0, 0)
                               for delta, status, data1, data2 in midi)
            self.cpu.mem_write(self.midi_buffer, header + payload)
            self.dispatch(25, pointer=self.midi_buffer)
        for pointer, payload in zip(self.audio_inputs, payloads):
            self.cpu.mem_write(pointer, payload)
        for pointer in self.audio_outputs:
            self.cpu.mem_write(pointer, bytes(frames * 4))  # legacy accumulating process is supported too
        self.call(self.audio_processor, self.effect, self.input_table, self.output_table, frames)
        output = [list(struct.unpack('<' + 'f' * frames, self.read(pointer, frames * 4)))
                  for pointer in self.audio_outputs]
        if any(not math.isfinite(value) for channel in output for value in channel):
            self.failed = True
            raise CompatibilityError('i386 plugin produced non-finite audio')
        self.position += frames
        self.phase = 'audio ready'
        return output

    def diagnostics(self):
        return dict(architecture='i386', phase=self.phase,
                    completed_initializers=self.completed_initializers,
                    missing_import=self.missing_import, import_counts=dict(self.import_counts),
                    optional_function_lookups=list(self.cf.lookups),
                    dynamic_symbol_lookups=list(self.darwin.dynamic_lookups),
                    host_calls=dict(self.host_calls), unknown_host_calls=sorted(self.unknown_host_calls),
                    captured_stdout=bytes(self.stdio.output[1]).decode('utf-8', errors='replace'),
                    captured_stderr=bytes(self.stdio.output[2]).decode('utf-8', errors='replace'),
                    file_queries=list(self.stdio.open_queries),
                    filesystem_queries=list(self.filesystem.queries),
                    resource_queries=list(self.resources.queries),
                    suppressed_alerts=list(self.resources.alerts),
                    instruction_pointer=self.cpu.reg_read(self.x86.UC_X86_REG_EIP),
                    rosetta_used=False, audio_compatibility_verified=False)

    def close(self, *, suppress=False):
        try:
            self._close()
        except Exception:
            if not suppress:
                raise

    def _close(self):
        if self.closed:
            return
        self.deadlines.append(time.monotonic() + 2)
        try:
            if not self.failed:
                if self.started:
                    self.phase = 'VST stop process'
                    self.started = False
                    self.dispatch(72)
                if self.active:
                    self.phase = 'VST mains off'
                    self.active = False
                    self.dispatch(12, value=0)
                if self.opened:
                    self.phase = 'VST close'
                    self.opened = False
                    self.dispatch(1)
                self.phase = 'exit handlers'
                # Remove each registration before calling it. A trap stops
                # guest cleanup; native objects are still released below.
                budget = 4096
                while self.darwin.exit_handlers:
                    if not budget:
                        raise CompatibilityError('Too many i386 exit callbacks')
                    budget -= 1
                    function, arguments, _ = self.darwin.exit_handlers.pop()
                    self.call(function, *arguments)
        finally:
            self.deadlines.pop()
            self.closed = True
            self.canvas.close()
            self.graphics.close()
            self.cf.close()
            self.stdio.close()
            self.filesystem.close()
