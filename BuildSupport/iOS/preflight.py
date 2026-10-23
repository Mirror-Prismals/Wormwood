#!/usr/bin/env python3
"""Read-only iPhone deployment preflight. Does not sign, install, or launch apps."""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def command(argv, timeout=20):
    try:
        run = subprocess.run(argv, text=True, capture_output=True, timeout=timeout)
        return {'ok': run.returncode == 0, 'exit_code': run.returncode,
                'stdout': run.stdout.strip(), 'stderr': run.stderr.strip()}
    except (subprocess.TimeoutExpired, OSError) as error:
        return {'ok': False, 'error': str(error)}

def main():
    report = {
        'xcode': command(['xcode-select', '-p']),
        'iphoneos_sdk': command(['xcrun', '--sdk', 'iphoneos', '--show-sdk-version']),
        'rust_targets': command(['rustup', 'target', 'list', '--installed']),
        'code_signing': command(['security', 'find-identity', '-v', '-p', 'codesigning']),
    }
    with tempfile.TemporaryDirectory(prefix='wormwood-ios-') as temporary:
        device_json = Path(temporary) / 'devices.json'
        report['device_query'] = command(['xcrun', 'devicectl', 'list', 'devices',
                                          '--timeout', '10', '--json-output', str(device_json)])
        if device_json.exists():
            report['devices'] = json.loads(device_json.read_text()).get('result', {}).get('devices', [])
    targets = report['rust_targets'].get('stdout', '').splitlines()
    report['rust_device_target_installed'] = 'aarch64-apple-ios' in targets
    # A paired device and signing identity are distinct prerequisites. An empty
    # identity list does not determine whether Xcode can provision a new one.
    report['valid_signing_identity_visible'] = any(
        '"Apple Development:' in line or '"iPhone Developer:' in line
        for line in report['code_signing'].get('stdout', '').splitlines())
    report['native_cue_implemented'] = False
    report['deployment_verified'] = False
    destination = ROOT / 'output' / 'ios-preflight.json'
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))

if __name__ == '__main__':
    main()
