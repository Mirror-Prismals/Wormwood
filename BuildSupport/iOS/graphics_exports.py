"""Reject iOS packages whose runtime graphics entry points were stripped."""
import json
import plistlib
import re
import subprocess


def required_graphics_symbols(root):
    required = set()
    for source in (root / 'Render').rglob('*.rs'):
        code = source.read_text()
        # Main generated API call sites and child clients' explicit loader tables.
        required.update(re.findall(r'\.((?:vk|wgpu)[A-Z][A-Za-z0-9_]+)', code))
        required.update(re.findall(r'\b((?:vk|wgpu)[A-Z][A-Za-z0-9_]+)\s*:\s*\(', code))
    return required

def verify_graphics_exports(root, app, report_path):
    required = required_graphics_symbols(root)
    retained = set(re.findall(r'&(vk[A-Za-z0-9_]+|wgpu[A-Za-z0-9_]+)',
                             (root / 'BaseSystem/LayerOne/IOSGraphicsSymbols.c').read_text()))
    executable = plistlib.loads((app / 'Info.plist').read_bytes())['CFBundleExecutable']
    output = subprocess.check_output(['xcrun', 'dyld_info', '-exports', str(app / executable)], text=True)
    exported = set(re.findall(r'\b_(vk[A-Za-z0-9_]+|wgpu[A-Za-z0-9_]+)\b', output))
    missing = sorted(required - exported)
    missing_retention = sorted(required - retained)
    passed = bool(required) and not missing and not missing_retention
    report = {'passed': passed, 'required_count': len(required),
              'exported_count': len(exported), 'missing_exports': missing,
              'missing_retention': missing_retention, 'device_rendering_verified': False}
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    if not passed:
        raise RuntimeError('iOS graphics entry points are unavailable: ' + json.dumps(report))
    return report
