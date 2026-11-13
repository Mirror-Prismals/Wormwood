"""Bounded Darwin C-locale operations, independent of the host process locale."""
from macho import CompatibilityError

SYMBOLS = ("_wctob", "_btowc")


def invoke(worker, name):
    value = worker.arg(0) & 0xffffffff
    if name in ("_wctob", "_btowc"):
        # Darwin's default NONE encoding is a one-byte identity encoding.
        # Values outside unsigned char, including EOF/WEOF, cannot convert.
        return value if value <= 255 else -1
    raise CompatibilityError(f"Unimplemented C-locale function: {name}")
