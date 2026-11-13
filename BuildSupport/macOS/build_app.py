#!/usr/bin/env python3
"""Build the local macOS application; external JACK/graphics installations stay external."""
from pathlib import Path
import argparse,plistlib,shutil,subprocess,json,os
ROOT=Path(__file__).resolve().parents[2]
def run(args):subprocess.run([str(a) for a in args],cwd=ROOT,check=True)
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output-dir',type=Path,default=ROOT/'output/macOS',
                        help='Stage the application separately before replacing the installed local build')
    parser.add_argument("--mod-package",type=Path,action="append",default=[],help="Install a separately built mod package, disabled by default")
    args=parser.parse_args()
    run(['cargo','build','--release','--offline','--locked'])
    project=json.loads((ROOT/'Project.json').read_text())
    app=args.output_dir.resolve()/f"{project['name']}.app";contents=app/'Contents';mac=contents/'MacOS';assets=contents/'Resources/EngineAssets'
    # Remove only generated contents so stale procedures/contracts cannot survive rebuilds.
    if contents.exists():shutil.rmtree(contents)
    mac.mkdir(parents=True);assets.mkdir(parents=True)
    target=Path(os.environ.get('CARGO_TARGET_DIR','target'))
    if not target.is_absolute():target=ROOT/target
    shutil.copy2(target/'release/wormwood',mac/'wormwood-engine')
    for name in ['Procedures','Systems','SimulationWorkshop']:shutil.copytree(ROOT/name,assets/name,ignore=shutil.ignore_patterns('__pycache__'))
    shutil.copytree(ROOT/'Platform/Vst2Compat',assets/'Platform/Vst2Compat',ignore=shutil.ignore_patterns('__pycache__','*.pyc'))
    (assets/'Mods').mkdir()
    (assets/'Mods/enabled.json').write_text('{"schema_version":1,"enabled":[]}\n')
    for package in args.mod_package:
        manifest=json.loads((package/'mod.json').read_text())
        identifier=manifest['id']
        if not identifier or any(c not in 'abcdefghijklmnopqrstuvwxyz0123456789-_' for c in identifier):
            raise ValueError('Invalid mod package id')
        destination=assets/'Mods'/identifier
        if destination.exists():raise ValueError(f'Duplicate mod package {identifier}')
        shutil.copytree(package,destination)
    # AUTHOR POLICY: ship immutable per-component lighting; never bake at launch.
    shutil.copytree(ROOT/'Art/AudioWorkstation/Baked',assets/'Art/AudioWorkstation/Baked')
    # Optional Cardinal 2D presets; 3D gem ruby remains the main/default theme.
    shutil.copytree(ROOT/'Art/AudioWorkstation/CardinalThemes',assets/'Art/AudioWorkstation/CardinalThemes',ignore=shutil.ignore_patterns('Reference'))
    # Package the runtime panel meshes, not Blender sources or studio previews.
    panel_path=Path('Art/AudioWorkstation/GemPanels')
    (assets/panel_path).mkdir(parents=True)
    for name in ['GemPanel_Broad.glb','GemPanel_Tall.glb','GemPanel_Compact.glb','GemPanel_Strip.glb','GemPanels_manifest.json','GemPanels_runtime.json','GemPanels_AgX.bin']:
        shutil.copy2(ROOT/panel_path/name,assets/panel_path/name)
    controls_path=Path('Art/AudioWorkstation/DecorativeControls')
    shutil.copytree(ROOT/controls_path,assets/controls_path)
    edison_path=Path('Art/AudioWorkstation/EdisonHardware')
    (assets/edison_path).mkdir(parents=True)
    for name in ['runtime.bin','runtime.json','connected.bin','connected.json']:
        shutil.copy2(ROOT/edison_path/name,assets/edison_path/name)
    engraving_path=Path('Art/AudioWorkstation/LaserEngraving')
    (assets/engraving_path).mkdir(parents=True)
    for name in ['engraving.json','engraving.bin','density.bin']:
        shutil.copy2(ROOT/engraving_path/name,assets/engraving_path/name)
    control_engraving_path=Path('Art/AudioWorkstation/ControlEngraving')
    (assets/control_engraving_path).mkdir(parents=True)
    for name in ['control_engraving.json','control_engraving.bin']:
        shutil.copy2(ROOT/control_engraving_path/name,assets/control_engraving_path/name)
    monitor_path=Path('Art/AudioWorkstation/InputMonitor')
    (assets/monitor_path).mkdir(parents=True)
    for name in ['runtime.json','runtime.bin']:
        shutil.copy2(ROOT/monitor_path/name,assets/monitor_path/name)
    arm_path=Path('Art/AudioWorkstation/CrosswalkArming')
    (assets/arm_path).mkdir(parents=True)
    for name in ['runtime.json','runtime.bin']:
        shutil.copy2(ROOT/arm_path/name,assets/arm_path/name)
    bubble_path=Path('Art/AudioWorkstation/BubbleNotes')
    (assets/bubble_path).mkdir(parents=True)
    for name in ['BubbleNotes_runtime.json','BubbleNotes_runtime.bin']:
        shutil.copy2(ROOT/bubble_path/name,assets/bubble_path/name)
    piano_path=Path('Art/AudioWorkstation/PianoKeyboard')
    (assets/piano_path).mkdir(parents=True)
    for name in ['PianoKeyboard_runtime.json','PianoKeyboard.glb']:
        shutil.copy2(ROOT/piano_path/name,assets/piano_path/name)
    timeline_path=Path('Art/AudioWorkstation/Timeline')
    (assets/timeline_path).mkdir(parents=True)
    for name in ['runtime.json','runtime.bin']:
        shutil.copy2(ROOT/timeline_path/name,assets/timeline_path/name)
    (assets/'BaseSystem').mkdir()
    for name in ['registry.json','circuit_breaker.json']:shutil.copy2(ROOT/'BaseSystem'/name,assets/'BaseSystem'/name)
    for name in ['LICENSE','THIRD_PARTY_NOTICES.md','Project.json']:shutil.copy2(ROOT/name,assets/name)
    shutil.copytree(ROOT/'BuildSupport/Licenses',assets/'ThirdPartyLicenses')
    shutil.copytree(ROOT/'Art/AudioWorkstation/BubbleNotes/Baked',assets/'Art/AudioWorkstation/BubbleNotes/Baked',ignore=shutil.ignore_patterns('frames'))
    info={'CFBundleExecutable':'Wormwood','CFBundleIdentifier':project['bundle_id'],'CFBundleName':project['name'],'CFBundleDisplayName':project['display_name'],'WormwoodDataDirectory':project['data_directory'],'DefaultScene':project['default_scene'],'EnvironmentLabel':project['environment_label'],'EnvironmentControls':project['environment_controls'],'CFBundlePackageType':'APPL','CFBundleShortVersionString':'0.1.0','CFBundleVersion':'1','LSMinimumSystemVersion':'13.0','NSHighResolutionCapable':True,'NSPrincipalClass':'NSApplication'}
    (contents/'Info.plist').write_bytes(plistlib.dumps(info))
    run(['xcrun','clang','-fobjc-arc','-framework','Cocoa',ROOT/'BuildSupport/macOS/Launcher.m','-o',mac/'Wormwood'])
    run(['codesign','--force','--sign','-',mac/'wormwood-engine'])
    run(['codesign','--force','--sign','-',app])
    run(['codesign','--verify','--strict',app])
    print(app)
if __name__=='__main__':main()
