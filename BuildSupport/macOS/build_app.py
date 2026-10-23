#!/usr/bin/env python3
"""Build the local macOS application; external JACK/graphics installations stay external."""
from pathlib import Path
import plistlib,shutil,subprocess,json,os
ROOT=Path(__file__).resolve().parents[2]
def run(args):subprocess.run([str(a) for a in args],cwd=ROOT,check=True)
def main():
    run(['cargo','build','--offline','--locked'])
    project=json.loads((ROOT/'Project.json').read_text())
    app=ROOT/'output/macOS'/f"{project['name']}.app";contents=app/'Contents';mac=contents/'MacOS';assets=contents/'Resources/EngineAssets'
    # Remove only generated contents so stale procedures/contracts cannot survive rebuilds.
    if contents.exists():shutil.rmtree(contents)
    mac.mkdir(parents=True);assets.mkdir(parents=True)
    shutil.copy2(Path(os.environ.get('CARGO_TARGET_DIR',str(ROOT/'target')))/'debug/wormwood',mac/'wormwood-engine')
    for name in ['Procedures','Systems']:shutil.copytree(ROOT/name,assets/name,ignore=shutil.ignore_patterns('__pycache__'))
    (assets/'BaseSystem').mkdir()
    for name in ['registry.json','circuit_breaker.json']:shutil.copy2(ROOT/'BaseSystem'/name,assets/'BaseSystem'/name)
    for name in ['LICENSE','THIRD_PARTY_NOTICES.md','Project.json']:shutil.copy2(ROOT/name,assets/name)
    shutil.copytree(ROOT/'BuildSupport/Licenses',assets/'ThirdPartyLicenses')
    info={'CFBundleExecutable':'Wormwood','CFBundleIdentifier':project['bundle_id'],'CFBundleName':project['name'],'CFBundleDisplayName':project['display_name'],'WormwoodDataDirectory':project['data_directory'],'DefaultScene':project['default_scene'],'EnvironmentLabel':project['environment_label'],'EnvironmentControls':project['environment_controls'],'CFBundlePackageType':'APPL','CFBundleShortVersionString':'0.1.0','CFBundleVersion':'1','LSMinimumSystemVersion':'13.0','NSHighResolutionCapable':True,'NSPrincipalClass':'NSApplication'}
    (contents/'Info.plist').write_bytes(plistlib.dumps(info))
    run(['xcrun','clang','-fobjc-arc','-framework','Cocoa',ROOT/'BuildSupport/macOS/Launcher.m','-o',mac/'Wormwood'])
    run(['codesign','--force','--sign','-',mac/'wormwood-engine'])
    run(['codesign','--force','--sign','-',app])
    run(['codesign','--verify','--strict',app])
    print(app)
if __name__=='__main__':main()
