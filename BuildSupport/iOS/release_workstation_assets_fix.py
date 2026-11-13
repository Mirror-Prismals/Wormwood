#!/usr/bin/env python3
"""Reproduce build 2495.19.10's missing DAW assets, then build/upload the fix."""
import os
from pathlib import Path
import plistlib
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def main():
    app = ROOT / 'output/ios-release/MirrorPrismals.xcarchive/Products/Applications/Wormwood.app'
    info = plistlib.loads((app / 'Info.plist').read_bytes())
    if info['CFBundleVersion'] != '2495.19.10':
        raise RuntimeError('The regression reproduction requires the original affected archive')
    evidence = ROOT / 'output/checks/ios-workstation-assets-2495.19.10'
    evidence.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.pop('WORMWOOD_STYLE', None)
    env['WORMWOOD_TEST_ASSET_ROOT'] = str(app / 'EngineAssets')
    log = evidence / 'missing-assets-regression.log'
    with log.open('w') as stream:
        result = subprocess.run(
            ['cargo', 'test', '--offline', '--locked', '--release', '--lib',
             'workstation_packaged_assets', '--', '--ignored'],
            cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
    output = log.read_text()
    if (result.returncode != 101 or '0 passed; 2 failed;' not in output
            or 'EdisonHardware/connected.json' not in output):
        raise RuntimeError(f'The expected missing-asset failure was not reproduced; see {log}')
    print('Both real workstation loaders rejected the affected archive as expected.', flush=True)
    subprocess.run([sys.executable, 'BuildSupport/iOS/build_ios.py', '--testflight', '--upload'],
                   cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
