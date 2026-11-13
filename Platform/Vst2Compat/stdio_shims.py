"""Bounded guest stdio and read-only regular files; no native FILE pointers."""
import os
from collections import deque
import stat
import struct
from macho import CompatibilityError
from libc_shims import release, string_bytes

DATA_NAMES = ("___stdinp", "___stdoutp", "___stderrp")
SYMBOLS = ("___error", "_fileno", "_fflush", "_fclose", "_fread", "_fwrite",
           "_fputs", "_puts", "_fputc", "_putc", "___swbuf", "_fgetc", "_getc",
           "___srget", "_ungetc", "_feof", "_ferror", "_clearerr", "_setvbuf",
           "_read", "_write", "_isatty", "_fseek", "_ftell", "_fopen", "_fscanf")
LIMIT = 65536


class Stdio:
    FILE_SIZE, FILE_STRIDE, POINTER_SIZE = 152, 160, 8
    FLAGS_OFFSET, READ_OFFSET, WRITE_OFFSET = 16, 8, 12

    def __init__(self, worker, base):
        self.worker, self.base = worker, base
        self.files = {base + 256 + fd * self.FILE_STRIDE: fd for fd in range(3)}
        self.closed = set()
        self.native_files = {}
        self.file_pushback = {}
        self.next_fd = 3
        self.open_queries = deque(maxlen=64)
        self.input = bytearray()
        self.output = {1: bytearray(), 2: bytearray()}
        self.pushback = []
        for pointer, fd in self.files.items():
            # FILE is 152 bytes on Darwin x86_64. Read/write counts stay zero,
            # forcing inline getc/putc through __srget/__swbuf rather than NULL.
            blob = bytearray(self.FILE_SIZE)
            struct.pack_into("<hh", blob, self.FLAGS_OFFSET, 2 | (4 if fd == 0 else 8), fd)
            worker.cpu.mem_write(pointer, bytes(blob))
            worker.cpu.mem_write(base + fd * self.POINTER_SIZE, pointer.to_bytes(self.POINTER_SIZE, 'little'))

    def open_file(self, path, mode):
        if mode not in (b"r", b"rb", b"rt"):
            if mode in (b"w", b"wb", b"wt", b"a", b"ab", b"at", b"r+", b"rb+", b"r+b", b"rt+", b"r+t",
                        b"w+", b"wb+", b"w+b", b"wt+", b"w+t", b"a+", b"ab+", b"a+b", b"at+", b"a+t"):
                self.errno(30)  # EROFS: this worker deliberately exposes read-only files.
                self.open_queries.append({"path": os.fsdecode(path), "mode": mode.decode(), "errno": 30})
                return 0
            raise CompatibilityError(f"Unsupported file mode {mode!r}")
        if len(self.native_files) >= 64 or self.next_fd > 32767:
            self.errno(24)  # EMFILE
            return 0
        native = None
        try:
            # Nonblocking open avoids hanging on FIFOs before fstat can reject them.
            native = os.open(self.worker.resource_paths.resolve(path), os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
            if not stat.S_ISREG(os.fstat(native).st_mode):
                raise OSError(22, "Only regular files are supported")
        except OSError as exc:
            if native is not None:
                os.close(native)
            self.errno(exc.errno)
            self.open_queries.append({"path": os.fsdecode(path), "mode": mode.decode(), "errno": exc.errno})
            return 0
        try:
            pointer = self.worker.allocate(self.FILE_SIZE)
            blob = bytearray(self.FILE_SIZE)
            struct.pack_into("<hh", blob, self.FLAGS_OFFSET, 6, self.next_fd)
            self.worker.cpu.mem_write(pointer, bytes(blob))
        except Exception:
            os.close(native)
            raise
        fd = self.next_fd
        self.next_fd += 1
        self.files[pointer] = fd
        self.native_files[fd] = native
        self.file_pushback[fd] = []
        self.open_queries.append({"path": os.fsdecode(path), "mode": mode.decode(), "errno": 0})
        return pointer

    def close(self):
        for pointer, fd in list(self.files.items()):
            if fd >= 3:
                if fd in self.native_files:
                    os.close(self.native_files.pop(fd))
                self.file_pushback.pop(fd, None)
                del self.files[pointer]
                release(self.worker, pointer)

    def read_file(self, fd, count, use_pushback=True):
        if fd == 0:
            return self.read_input(count, use_pushback)
        if fd not in self.native_files:
            raise OSError(9, "Bad file descriptor")
        data = bytearray()
        pending = self.file_pushback[fd]
        while use_pushback and pending and len(data) < count:
            data.append(pending.pop())
        data.extend(os.read(self.native_files[fd], count - len(data)))
        return bytes(data)

    def errno(self, value):
        self.worker.cpu.mem_write(self.base + 24, struct.pack("<i", value))

    def flags(self, pointer, set_bits=0, clear_bits=0):
        flags, = struct.unpack("<H", self.worker.read(pointer + self.FLAGS_OFFSET, 2))
        self.worker.cpu.mem_write(pointer + self.FLAGS_OFFSET, struct.pack("<H", (flags | set_bits) & ~clear_bits))
        return flags

    def stream(self, pointer):
        if pointer not in self.files or self.files[pointer] in self.closed:
            raise CompatibilityError("Unsupported or closed guest FILE object")
        return self.files[pointer]

    def read_input(self, count, use_pushback=True):
        data = bytearray()
        while use_pushback and self.pushback and len(data) < count:
            data.append(self.pushback.pop())
        remaining = min(count - len(data), len(self.input))
        data.extend(self.input[:remaining])
        del self.input[:remaining]
        return bytes(data)

    def write_output(self, fd, data):
        if fd not in self.output or fd in self.closed:
            self.errno(9)  # EBADF
            return -1
        if len(self.output[fd]) + len(data) > LIMIT:
            raise CompatibilityError("Guest standard-output capture exceeds 64 KiB")
        self.output[fd].extend(data)
        return len(data)

    def invoke(self, name):
        w = self.worker
        a, b, c, d = (w.arg(i) for i in range(4))
        if name == "_fscanf":
            from scan_shims import scan
            return scan(self, a, b)
        if name == "_fopen":
            return self.open_file(string_bytes(w, a, limit=4096), string_bytes(w, b, limit=16))
        if name == "___error":
            return self.base + 24
        if name in ("_read", "_write", "_isatty"):
            fd = a & 0xffffffff
            if name == "_isatty":
                self.errno(25 if (fd in range(3) or fd in self.native_files) and fd not in self.closed else 9)
                return 0
            if c > 4 * 1024 * 1024:
                raise CompatibilityError("Guest stdio transfer exceeds operation limit")
            if name == "_write":
                return self.write_output(fd, w.read(b, c))
            try:
                if fd in self.closed:
                    raise OSError(9, "Closed descriptor")
                data = self.read_file(fd, c, use_pushback=False)
            except OSError as exc:
                self.errno(exc.errno)
                return -1
            if data:
                w.cpu.mem_write(b, data)
            return len(data)
        if name == "_puts":
            return 0 if self.write_output(1, string_bytes(w, a) + b"\n") >= 0 else -1
        if name == "_fflush" and not a:
            return 0  # all streams are unbuffered, already captured
        pointer = d if name in ("_fread", "_fwrite") else b if name in (
            "_fputs", "_fputc", "_putc", "___swbuf", "_ungetc") else a
        fd = self.stream(pointer)
        if name == "_fileno":
            return fd
        if name == "_fflush":
            return 0
        if name == "_fclose":
            if fd in self.native_files:
                os.close(self.native_files.pop(fd))
                self.file_pushback.pop(fd)
            self.closed.add(fd)
            self.flags(pointer, clear_bits=0xffff)
            return 0
        if name in ("_feof", "_ferror"):
            return int(bool(self.flags(pointer) & (0x20 if name == "_feof" else 0x40)))
        if name == "_clearerr":
            self.flags(pointer, clear_bits=0x60)
            return 0
        if name == "_setvbuf":
            return 0 if c == 2 else -1  # only _IONBF is supported
        if name in ("_fseek", "_ftell"):
            if fd in self.native_files:
                try:
                    pending = self.file_pushback[fd]
                    if name == "_ftell":
                        position = os.lseek(self.native_files[fd], 0, os.SEEK_CUR) - len(pending)
                        if position >= 1 << (self.POINTER_SIZE * 8 - 1):
                            raise OSError(84, 'File position exceeds guest long')  # Darwin EOVERFLOW
                        return position
                    bits = self.POINTER_SIZE * 8
                    offset = b - (1 << bits) if b >> (bits - 1) else b
                    if c not in (0, 1, 2):
                        raise OSError(22, "Invalid seek origin")
                    os.lseek(self.native_files[fd], offset - (len(pending) if c == 1 else 0), c)
                    pending.clear()
                    self.flags(pointer, clear_bits=0x20)
                    w.cpu.mem_write(pointer + self.READ_OFFSET, bytes(4))
                    return 0
                except OSError as exc:
                    self.errno(exc.errno)
                    return -1
            self.errno(29)  # ESPIPE: captured standard streams are not seekable
            return -1
        if name == "_ungetc":
            if (fd != 0 and fd not in self.native_files) or a & 0xffffffff == 0xffffffff:
                return -1
            pending = self.pushback if fd == 0 else self.file_pushback[fd]
            if len(pending) >= LIMIT:
                raise CompatibilityError("Guest stdin pushback exceeds limit")
            pending.append(a & 255)
            self.flags(pointer, clear_bits=0x20)
            return a & 255
        if name in ("_fputc", "_putc", "___swbuf", "_fputs", "_fwrite"):
            if name == "_fwrite":
                if not b or not c:
                    return 0
                data = w.read(a, b * c)
            elif name == "_fputs":
                data = string_bytes(w, a)
            else:
                data = bytes([a & 255])
            result = self.write_output(fd, data)
            if result < 0:
                self.flags(pointer, set_bits=0x40)
                return 0 if name == "_fwrite" else -1
            w.cpu.mem_write(pointer + self.WRITE_OFFSET, bytes(4))  # reset inline putc count
            if name == "_fwrite":
                return c
            return 0 if name == "_fputs" else a & 255
        if name in ("_fgetc", "_getc", "___srget", "_fread"):
            count = b * c if name == "_fread" else 1
            if count > 4 * 1024 * 1024:
                raise CompatibilityError("Guest stdio transfer exceeds operation limit")
            if not count:
                return 0
            try:
                data = b"" if self.flags(pointer) & 0x20 else self.read_file(fd, count)
            except OSError as exc:
                self.errno(exc.errno)
                self.flags(pointer, set_bits=0x40)
                return 0 if name == "_fread" else -1
            w.cpu.mem_write(pointer + self.READ_OFFSET, bytes(4))  # reset inline getc count
            if len(data) < count:
                self.flags(pointer, set_bits=0x20)
            if name == "_fread":
                if data:
                    w.cpu.mem_write(a, data)
                return len(data) // b
            return data[0] if data else -1
        raise CompatibilityError(f"Unimplemented stdio function: {name}")
