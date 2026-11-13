#!/usr/bin/env bash
# Optional, desktop-only dependency for Cardinal's isolated Intel VST2 worker.
# Native VST3/CLAP/AU need no Python runtime. No game build or plugin execution.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  echo 'The retained VST2 compatibility worker requires Apple Silicon macOS.' >&2
  exit 1
fi
python3 -m venv "$ROOT/build/vst2-compat/venv"
"$ROOT/build/vst2-compat/venv/bin/python" -m pip install -r "$ROOT/Platform/Vst2Compat/requirements.txt"
