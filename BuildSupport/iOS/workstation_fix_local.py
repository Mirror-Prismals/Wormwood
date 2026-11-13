#!/usr/bin/env python3
"""Check the real panel models, then prepare a local signed iPhone archive."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[2]
paths = [ROOT / 'BaseSystem/LayerFour/WorkstationPanelSystem.rs', ROOT / 'Workstation/GemMesh.rs'] + sorted((ROOT / 'Art/AudioWorkstation/GemPanels').glob('GemPanel_*.glb'))
def digests():
    return {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
before = digests()
print('Checking all four actual panel meshes.', flush=True)
subprocess.run(['cargo', 'test', '--offline', '--locked', '--release', '--lib', 'workstation_actual_panels_pass_convex_validation_and_make_scene', '--', '--nocapture'], cwd=ROOT, check=True)
if digests() != before:
    raise RuntimeError('Panel source or assets changed during validation')
print('Preparing the signed iPhone archive locally; no upload.', flush=True)
subprocess.run([sys.executable, 'BuildSupport/iOS/build_ios.py', '--wait-for-build', '--testflight'], cwd=ROOT, check=True)
report = json.loads((ROOT / 'output/ios-release/build-report.json').read_text())
if not report.get('signed') or not report.get('archived') or report.get('uploaded'):
    raise RuntimeError('Local signed archive was not confirmed')
print('Prepared signed local build ' + report['build_number'] + '; upload remains pending.', flush=True)
