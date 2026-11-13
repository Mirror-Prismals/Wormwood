"""Darwin i386 FILE layout for the shared bounded, read-only stream layer."""
import stdio_shims

# fscanf has a separate AMD64 varargs reader; don't advertise it on i386 yet.
SYMBOLS = tuple(name for name in stdio_shims.SYMBOLS if name != '_fscanf') + ('_fprintf', '_mkdir', '_rmdir')


class Stdio32(stdio_shims.Stdio):
    FILE_SIZE, FILE_STRIDE, POINTER_SIZE = 88, 88, 4
    FLAGS_OFFSET, READ_OFFSET, WRITE_OFFSET = 12, 4, 8

    def invoke(self, name):
        if name in ('_mkdir', '_rmdir'):
            from libc_shims import string_bytes
            path = string_bytes(self.worker, self.worker.arg(0), limit=4096)
            self.errno(30)  # EROFS: report failure, never pretend a write succeeded
            self.open_queries.append(dict(path=path.decode('utf-8', errors='replace'), mode=name, errno=30))
            return -1
        if name == '_fprintf':
            from format_shims import format_bytes
            from format32 import Arguments32
            w = self.worker
            payload = format_bytes(w, w.arg(1), Arguments32(w, 2))
            return self.write_output(self.stream(w.arg(0)), payload)
        return super().invoke(name)
