#!/usr/bin/env python3
"""Run library checks, then build/archive/upload this game's current playtest.

Finite foreground payload for Codex Process Jobs. Never reads authentication
secrets; build_ios.py uses the existing signing/account configuration.
"""
from pathlib import Path
import datetime
import hashlib
import json
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'BuildSupport'))
from checked_process import run_checked
EVIDENCE = ROOT / 'output/checks' / ('workstation-panels-' +
    datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S-%f'))


def run(command, log, timeout=360):
    print('Running ' + command[0] + ' — ' + log.name, flush=True)
    run_checked(command, cwd=ROOT, log=log, timeout=timeout)


def source_manifest():
    files = {}
    folders = ['BaseSystem', 'Host', 'Physics', 'Terrain', 'Vegetation', 'Water',
               'Audio', 'Render', 'Platform', 'Systems', 'Procedures', 'BuildSupport', 'Workstation']
    sources = [p for folder in folders for p in (ROOT / folder).rglob('*') if p.is_file()]
    sources += [ROOT / name for name in ['Cargo.toml', 'Cargo.lock', 'build.rs', 'lib.rs',
                                        'BaseEntity.rs', 'Project.json']]
    panels = ROOT / 'Art/AudioWorkstation/GemPanels'
    sources += [panels / name for name in ['GemPanel_Broad.glb', 'GemPanel_Tall.glb', 'GemPanel_Compact.glb', 'GemPanel_Strip.glb', 'GemPanels_manifest.json', 'GemPanels_runtime.json', 'GemPanels_AgX.bin']]
    for path in sorted(sources):
        if '__pycache__' in path.parts or path.name == '.DS_Store':
            continue
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(block)
        files[str(path.relative_to(ROOT))] = digest.hexdigest()
    return files


def main():
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    print('Release evidence: ' + str(EVIDENCE), flush=True)
    state = {'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
             'phase': 'library_tests', 'uploaded': False, 'gameplay_tested': False}
    try:
        manifest = source_manifest()
        (EVIDENCE / 'source-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        # Exercise the release-optimized solver: the rooted-soil tests enforce
        # their own wall-clock bounds, which debug-mode solver code can exceed.
        run(['cargo', 'test', '--offline', '--locked', '--release', '--lib', '--', '--test-threads=1'],
            EVIDENCE / 'library-tests.log')
        state['library_tests_passed'] = True
        if source_manifest() != manifest:
            raise RuntimeError('Sources changed during checks; stop before building a different revision')
        state['phase'] = 'signed_archive_and_testflight_upload'
        (EVIDENCE / 'release-attempt.json').write_text(json.dumps(state, indent=2) + '\n')
        run([sys.executable, 'BuildSupport/iOS/build_ios.py', '--wait-for-build',
             '--testflight', '--upload'], EVIDENCE / 'build-upload.log', timeout=1800)
        report = json.loads((ROOT / 'output/ios-release/build-report.json').read_text())
        if report.get('uploaded') is not True:
            raise RuntimeError('The build completed without confirming TestFlight upload')
        state.update({'phase': 'uploaded', 'uploaded': True,
                      'build_number': report['build_number'],
                      'apple_processing_verified': False, 'tester_availability_verified': False})
        release = ROOT / 'output/releases' / report['build_number']
        release.mkdir(parents=True, exist_ok=False)
        for filename in ['source-manifest.json', 'library-tests.log', 'build-upload.log',
                         'isolated-tests.txt', 'ios-rust-metadata.txt', 'uikit-syntax.txt', 'intent.json']:
            path = EVIDENCE / filename
            if path.exists():
                shutil.copy2(path, release / filename)
        shutil.copy2(ROOT / 'output/ios-release/build-report.json', release / 'build-report.json')
        for filename in ['rust.log', 'archive.log', 'export.log', 'codesign.log', 'graphics-exports.json']:
            path = ROOT / 'output/ios-release' / filename
            if path.exists():
                shutil.copy2(path, release / filename)
        (release / 'release-status.json').write_text(json.dumps(state, indent=2) + '\n')
        (release / 'mobile-controls.txt').write_text(
            'Every play session: skippable controls tutorial; replay from pause.\n'
            'Left drag moves; right drag looks; bottom-right jumps.\n'
            'Top-right corner pauses: Cheats, Flight and Noclip switches apply on Resume.\n'
            'Flight: bottom-right rises, bottom-left descends; double-tap left toggles fast movement.\n'
            'Noclip follows look direction. Turning it off in solid rock returns to a safe position.\n'
            'Cheats start off each session. Tutorial HUD disappears after Done or Skip.\n')
        with (release / 'mobile-controls.txt').open('a') as controls:
            controls.write('Workshop: Audio workstation · 3D panels. Swipe down/up/right/left to toggle the top/bottom/left/right panel. Two-finger double-tap exits.\n')
        print('Uploaded Mirror build ' + report['build_number'] + ' to TestFlight.', flush=True)
        print('Apple processing and tester availability have not been verified.', flush=True)
    except Exception as error:
        state['error'] = str(error)
        print('Release stopped: ' + str(error) + '. See ' + str(EVIDENCE), file=sys.stderr, flush=True)
        raise
    finally:
        (EVIDENCE / 'release-attempt.json').write_text(json.dumps(state, indent=2) + '\n')


if __name__ == '__main__':
    main()
