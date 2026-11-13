"""Optional read-only resource overlay for disposable compatibility diagnostics."""
import os
from pathlib import Path


class ResourcePaths:
    def __init__(self, root=None):
        self.root = Path(root).resolve(strict=True) if root else None
        if self.root is not None and not self.root.is_dir():
            raise ValueError("Resource root must be a directory")

    def resolve(self, path):
        if self.root is None or not os.path.isabs(path):
            return path
        relative = os.path.normpath(os.fsdecode(path)).lstrip('/')
        candidate = (self.root / relative).resolve()
        # Symlinks must not redirect overlay lookup outside its configured root.
        if candidate.is_relative_to(self.root) and candidate.exists():
            return os.fsencode(candidate) if isinstance(path, bytes) else str(candidate)
        return path
