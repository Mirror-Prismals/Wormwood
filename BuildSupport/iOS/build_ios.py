#!/usr/bin/env python3
"""Build the native iPhone app and validate its packaged workstation assets."""
from pathlib import Path
import argparse,datetime,fcntl,json,os,plistlib,shutil,subprocess,sys
from setup_deps import setup
from graphics_exports import verify_graphics_exports
from verify_startup_stack import verify as verify_startup_stack
ROOT=Path(__file__).resolve().parents[2]
def run(argv,log,env=None):
    print('Running '+str(argv[0])+' — '+log.name,flush=True)
    with log.open('w') as stream:
        subprocess.run([str(x) for x in argv],cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,env=env,check=True)
def verify_workstation_assets(app,out):
    # Exercise the actual CPU loaders against the finished app, never the checkout.
    env=os.environ.copy();env.pop('WORMWOOD_STYLE',None)
    env['WORMWOOD_TEST_ASSET_ROOT']=str(app/'EngineAssets')
    log=out/'workstation-assets.log'
    run(['cargo','test','--offline','--locked','--release','--lib','workstation_packaged_assets','--','--ignored'],log,env)
    if '2 passed; 0 failed;' not in log.read_text():
        raise RuntimeError('The packaged workstation loader checks did not both run')
    return {'passed':True,'loader_checks':2,'device_rendering_verified':False}
def icons(destination,water):
    # Original development icon, opaque RGB. No artwork or dependencies borrowed.
    import struct,zlib,math
    destination.mkdir(parents=True,exist_ok=True)
    def png(n):
        rows=[]
        for y in range(n):
            row=bytearray()
            for x in range(n):
                u,v=(x+.5)/n,(y+.5)/n
                if water:
                    mark=(abs(v-(.5+.10*math.sin(u*math.tau*1.4)))<.025 or abs(v-(.67+.07*math.sin(u*math.tau*1.4)))<.018) and .17<u<.83
                    color=(98,229,215) if mark else (7,36,54)
                else:
                    mark=.22<v<.78 and abs(u-.5)<(v-.2)*.56 and (abs(u-.5)>(v-.2)*.42 or v>.72)
                    color=(213,168,244) if mark else (29,18,48)
                row.extend(color)
            rows.append(b'\0'+row)
        def chunk(k,data):return struct.pack('>I',len(data))+k+data+struct.pack('>I',zlib.crc32(k+data)&0xffffffff)
        return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',n,n,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(b''.join(rows)))+chunk(b'IEND',b'')
    images=[]
    for idiom,sizes,scales in [('iphone',[20,29,40,60],[2,3]),('ipad',[20,29,40,76],[1,2]),('ipad',[83.5],[2]),('ios-marketing',[1024],[1])]:
        for size in sizes:
            for scale in scales:
                n=int(size*scale);filename=f'icon-{n}.png';(destination/filename).write_bytes(png(n));images.append({'idiom':idiom,'size':f'{size}x{size}','scale':f'{scale}x','filename':filename})
    (destination/'Contents.json').write_text(json.dumps({'images':images,'info':{'version':1,'author':'xcode'}},indent=2))
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--wait-for-build',action='store_true',help='Queue behind another build of this game')
    p.add_argument('--output-dir',type=Path,help='Separate generated candidate/archive directory; leaves the normal iOS output unchanged')
    p.add_argument('--team');p.add_argument('--device');p.add_argument('--build-number');p.add_argument('--testflight',action='store_true');p.add_argument('--upload',action='store_true');p.add_argument('--install',action='store_true');p.add_argument('--launch',action='store_true');p.add_argument('--unsigned',action='store_true')
    p.add_argument('--authentication-key-path');p.add_argument('--authentication-key-id');p.add_argument('--authentication-key-issuer-id')
    a=p.parse_args()
    if a.upload and not a.testflight:p.error('--upload requires --testflight')
    if a.unsigned and(a.upload or a.install or a.testflight):p.error('--unsigned is only for a local compilation/package build')
    if a.launch and not a.install:p.error('--launch requires --install')
    if a.install and a.testflight:p.error('Use TestFlight on the phone to install a distributed build')
    config_path=ROOT/'output/ios-signing.json';config=json.loads(config_path.read_text()) if config_path.exists() else {}
    team=a.team or os.environ.get('APPLE_DEVELOPMENT_TEAM') or config.get('development_team')
    device=a.device or config.get('device_identifier')
    if not team and not a.unsigned:p.error('Supply --team or APPLE_DEVELOPMENT_TEAM for your paid Apple Developer team')
    if a.install and not device:p.error('--install requires --device or device_identifier in output/ios-signing.json')
    keys=[a.authentication_key_path,a.authentication_key_id,a.authentication_key_issuer_id]
    if any(keys) and not all(keys):p.error('Supply all three authentication-key arguments together')
    auth=['-allowProvisioningUpdates'] if not a.unsigned else []
    if all(keys):auth+=['-authenticationKeyPath',keys[0],'-authenticationKeyID',keys[1],'-authenticationKeyIssuerID',keys[2]]
    project=json.loads((ROOT/'Project.json').read_text());out=(a.output_dir.resolve() if a.output_dir else ROOT/'output/ios-release');out.mkdir(parents=True,exist_ok=True)
    with(out/'build.lock').open('w') as lock:
        if a.wait_for_build:print("Waiting for this game’s previous build",flush=True)
        fcntl.flock(lock,fcntl.LOCK_EX if a.wait_for_build else fcntl.LOCK_EX|fcntl.LOCK_NB)
        now=datetime.datetime.now(datetime.timezone.utc);number_path=out/'last-build-number.txt'
        numeric=(now.date()-datetime.date(2020,1,1)).days*10000+now.hour*100+now.minute
        if number_path.exists():numeric=max(numeric,int(number_path.read_text())+1)
        number=a.build_number or f'{numeric//10000}.{numeric//100%100}.{numeric%100}'
        if not a.build_number:number_path.write_text(str(numeric))
        report={'app':project['display_name'],'bundle_id':project['ios_bundle_id'],'build_number':number,'team':team,'compiled':False,'archived':False,'signed':False,'uploaded':False,'installed':False,'gameplay_tested':False}
        try:
            sdk=setup();env=os.environ.copy();env['WORMWOOD_WEBGPU_INCLUDE']=sdk['wgpu_include'];env.setdefault('CARGO_BUILD_JOBS','1');env['CARGO_PROFILE_RELEASE_DEBUG']='line-tables-only'
            run(['cargo','build','--offline','--locked','--release','--lib','--target','aarch64-apple-ios'],out/'rust.log',env);report['compiled']=True
            report['startup_stack']=verify_startup_stack(out/'startup-stack.json')
            assets=out/'EngineAssets'
            if assets.exists():shutil.rmtree(assets)
            assets.mkdir()
            for folder in ['Procedures','Systems']:shutil.copytree(ROOT/folder,assets/folder)
            (assets/'Mods').mkdir();(assets/'Mods/enabled.json').write_text('{"schema_version":1,"enabled":[]}\n')
            # Same immutable production artwork as the verified desktop bundle.
            for folder in ['Baked','CardinalThemes','BubbleNotes/Baked']:
                shutil.copytree(ROOT/'Art/AudioWorkstation'/folder,assets/'Art/AudioWorkstation'/folder,ignore=shutil.ignore_patterns('Reference','__pycache__'))
            piano=Path('Art/AudioWorkstation/PianoKeyboard');(assets/piano).mkdir(parents=True)
            for file in ['PianoKeyboard_runtime.json','PianoKeyboard.glb']:
                shutil.copy2(ROOT/piano/file,assets/piano/file)
            # Package the runtime panel meshes, not Blender sources or studio previews.
            panel_path=Path('Art/AudioWorkstation/GemPanels')
            (assets/panel_path).mkdir(parents=True)
            for file in ['GemPanel_Broad.glb','GemPanel_Tall.glb','GemPanel_Compact.glb','GemPanel_Strip.glb','GemPanels_manifest.json','GemPanels_runtime.json','GemPanels_AgX.bin']:
                shutil.copy2(ROOT/panel_path/file,assets/panel_path/file)
            controls_path=Path('Art/AudioWorkstation/DecorativeControls')
            shutil.copytree(ROOT/controls_path,assets/controls_path)
            edison_path=Path('Art/AudioWorkstation/EdisonHardware')
            (assets/edison_path).mkdir(parents=True)
            for file in ['runtime.json','runtime.bin','connected.json','connected.bin']:
                shutil.copy2(ROOT/edison_path/file,assets/edison_path/file)
            engraving_path=Path('Art/AudioWorkstation/LaserEngraving')
            (assets/engraving_path).mkdir(parents=True)
            for file in ['engraving.json','engraving.bin','density.bin']:
                shutil.copy2(ROOT/engraving_path/file,assets/engraving_path/file)
            control_engraving_path=Path('Art/AudioWorkstation/ControlEngraving')
            (assets/control_engraving_path).mkdir(parents=True)
            for file in ['control_engraving.json','control_engraving.bin']:
                shutil.copy2(ROOT/control_engraving_path/file,assets/control_engraving_path/file)
            monitor_path=Path('Art/AudioWorkstation/InputMonitor')
            (assets/monitor_path).mkdir(parents=True)
            for file in ['runtime.json','runtime.bin']:
                shutil.copy2(ROOT/monitor_path/file,assets/monitor_path/file)
            arm_path=Path('Art/AudioWorkstation/CrosswalkArming')
            (assets/arm_path).mkdir(parents=True)
            for file in ['runtime.json','runtime.bin']:
                shutil.copy2(ROOT/arm_path/file,assets/arm_path/file)
            bubble_path=Path('Art/AudioWorkstation/BubbleNotes')
            (assets/bubble_path).mkdir(parents=True,exist_ok=True)
            for file in ['BubbleNotes_runtime.json','BubbleNotes_runtime.bin']:
                shutil.copy2(ROOT/bubble_path/file,assets/bubble_path/file)
            timeline_path=Path('Art/AudioWorkstation/Timeline')
            (assets/timeline_path).mkdir(parents=True)
            for file in ['runtime.json','runtime.bin']:
                shutil.copy2(ROOT/timeline_path/file,assets/timeline_path/file)
            (assets/'BaseSystem').mkdir()
            for file in ['registry.json','circuit_breaker.json']:shutil.copy2(ROOT/'BaseSystem'/file,assets/'BaseSystem'/file)
            shutil.copy2(ROOT/'Project.json',assets/'Project.json')
            notices=assets/'ThirdPartyLicenses';shutil.copytree(ROOT/'BuildSupport/Licenses',notices/'common')
            for file in ['LICENSE','THIRD_PARTY_NOTICES.md']:shutil.copy2(ROOT/file,notices/file)
            for component in ['wgpu','moltenvk']:
                for file in (ROOT/'output/ios-sdk'/component).rglob('*'):
                    if file.is_file() and any(s in file.name.lower() for s in ['license','licensing','copyright','notice']):
                        dest=notices/component/file.relative_to(ROOT/'output/ios-sdk'/component);dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(file,dest)
            catalog=out/'AppAssets.xcassets';icons(catalog/'AppIcon.appiconset',project['engine']=='Martin')
            (catalog/'Contents.json').write_text(json.dumps({'info':{'version':1,'author':'xcode'}}))
            generated=out/'project'
            run(['cmake','-S',ROOT/'BuildSupport/iOS','-B',generated,'-G','Xcode','-DCMAKE_SYSTEM_NAME=iOS','-DCMAKE_OSX_SYSROOT=iphoneos','-DCMAKE_OSX_ARCHITECTURES=arm64','-DCMAKE_OSX_DEPLOYMENT_TARGET=16.0',f'-DWORMWOOD_TEAM={team or ""}',f'-DWORMWOOD_BUILD_NUMBER={number}',f'-DWORMWOOD_BUNDLE_ID={project["ios_bundle_id"]}',f'-DWORMWOOD_ASSETS={assets}',f'-DWORMWOOD_APP_ASSETS={catalog}',f'-DWORMWOOD_RUST_LIBRARY={ROOT}/target/aarch64-apple-ios/release/libwormwood.a',f'-DWORMWOOD_WGPU_LIBRARY={sdk["wgpu_library"]}',f'-DWORMWOOD_MOLTENVK_LIBRARY={sdk["moltenvk_library"]}',f'-DWORMWOOD_WGPU_INCLUDE={sdk["wgpu_include"]}'],out/'configure.log')
            command=['xcodebuild','-project',generated/'WormwoodIOS.xcodeproj','-scheme','Wormwood','-configuration','Release','-destination','generic/platform=iOS','-jobs','1']+auth
            if a.unsigned:command+=['CODE_SIGNING_ALLOWED=NO','CODE_SIGNING_REQUIRED=NO']
            if a.testflight:
                archive=out/(project['name']+'.xcarchive')
                run(command+['-archivePath',archive,'archive'],out/'archive.log')
                metadata=plistlib.loads((archive/'Info.plist').read_bytes())
                properties=metadata.get('ApplicationProperties',{})
                application=properties.get('ApplicationPath')
                if not application or properties.get('CFBundleIdentifier')!=project['ios_bundle_id']:
                    raise RuntimeError('Xcode produced a generic or mismatched archive; expected this game under Products/Applications')
                app=(archive/'Products'/application).resolve()
                if not app.is_relative_to((archive/'Products/Applications').resolve()) or not app.is_dir():
                    raise RuntimeError('The archived app is missing from Products/Applications')
                report['archived']=True
                # Reject another upload with missing/mismatched native crash symbols.
                dsym=archive/'dSYMs'/f'{app.name}.dSYM'
                # CMake emits dSYMs in its configuration build directory rather
                # than Xcode's archive staging directory. Include them explicitly.
                if not dsym.exists():
                    generated_dsym=generated/'Release-iphoneos'/f'{app.name}.dSYM'
                    if not generated_dsym.is_dir():raise RuntimeError('Xcode did not generate iOS crash symbols')
                    dsym.parent.mkdir(parents=True,exist_ok=True)
                    shutil.copytree(generated_dsym,dsym)
                run(['xcrun','dwarfdump','--uuid',app/'Wormwood'],out/'binary-uuid.log')
                run(['xcrun','dwarfdump','--uuid',dsym],out/'dsym-uuid.log')
                import re
                binary_ids=set(re.findall(r'UUID: ([0-9A-Fa-f-]+)',(out/'binary-uuid.log').read_text()))
                symbol_ids=set(re.findall(r'UUID: ([0-9A-Fa-f-]+)',(out/'dsym-uuid.log').read_text()))
                if not binary_ids or not binary_ids.issubset(symbol_ids):raise RuntimeError('Missing matching iOS crash symbols')
                report['crash_symbols']={'matched':True,'uuids':sorted(binary_ids)}
                report['graphics_exports']=verify_graphics_exports(ROOT,app,out/'graphics-exports.json')
                report['workstation_assets']=verify_workstation_assets(app,out)
                run(['codesign','--verify','--strict',app],out/'codesign.log');report['signed']=True
                export={'method':'app-store-connect','destination':'upload' if a.upload else 'export','teamID':team,'signingStyle':'automatic','uploadSymbols':True,'testFlightInternalTestingOnly':True,'manageAppVersionAndBuildNumber':False}
                options=out/'ExportOptions.plist';options.write_bytes(plistlib.dumps(export))
                run(['xcodebuild','-exportArchive','-archivePath',archive,'-exportPath',out/'TestFlight','-exportOptionsPlist',options]+auth,out/'export.log');report['uploaded']=a.upload
            else:
                run(command+['build'],out/'xcode.log');app=generated/'Release-iphoneos/Wormwood.app';report['app_path']=str(app)
                report['graphics_exports']=verify_graphics_exports(ROOT,app,out/'graphics-exports.json')
                report['workstation_assets']=verify_workstation_assets(app,out)
                if not a.unsigned:run(['codesign','--verify','--strict',app],out/'codesign.log');report['signed']=True
                if a.install:
                    run(['xcrun','devicectl','device','install','app','--device',device,app],out/'install.log');report['installed']=True
                    if a.launch:run(['xcrun','devicectl','device','process','launch','--device',device,'--terminate-existing',project['ios_bundle_id']],out/'launch.log')
        except Exception as error:
            report['error']=str(error);print(f'Build stopped: {error}. Logs: {out}',file=sys.stderr);raise
        finally:
            (out/'build-report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report,indent=2))
if __name__=='__main__':main()
