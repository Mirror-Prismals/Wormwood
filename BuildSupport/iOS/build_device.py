#!/usr/bin/env python3
"""Build/sign/deploy the iOS Rust host and retrieve its on-device validation."""
import argparse,hashlib,json,plistlib,shutil,subprocess,time
from setup_deps import setup
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]

def execute(args,log=None,timeout=180):
    print('Running:', ' '.join(str(a) for a in args),flush=True)
    if log:
        with log.open('w') as stream:
            result=subprocess.run([str(a) for a in args],cwd=ROOT,stdout=stream,stderr=subprocess.STDOUT,timeout=timeout)
    else:
        result=subprocess.run([str(a) for a in args],cwd=ROOT,timeout=timeout)
    if result.returncode:raise RuntimeError(f'Command failed ({result.returncode}); log: {log}')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--team');parser.add_argument('--device');parser.add_argument('--build-only',action='store_true')
    parser.add_argument('--signing-timeout',type=int,default=300,help='Time allowed for Xcode build and manual system keychain approval')
    args=parser.parse_args()
    config_path=ROOT/'output/ios-signing.json'
    config=json.loads(config_path.read_text()) if config_path.exists() else {}
    team=args.team or config.get('development_team');device=args.device or config.get('device_identifier')
    if not team or (not args.build_only and not device):parser.error('Supply --team and --device, or local output/ios-signing.json')
    output=ROOT/'output/ios-device-build';output.mkdir(parents=True,exist_ok=True)
    result_path=output/'verification.json';result_path.unlink(missing_ok=True)
    result={'signed_build_verified':False,'deployed':False,'rust_host_verified_on_device':False,'graphics_verified':False,'audio_verified':False}
    try:
        files={}
        for directory in ['BaseSystem','Host','Platform','Procedures','Systems','Render','BuildSupport/iOS']:
            for path in sorted((ROOT/directory).rglob('*')):
                if path.is_file() and '__pycache__' not in path.parts:files[path.relative_to(ROOT).as_posix()]=hashlib.sha256(path.read_bytes()).hexdigest()
        for name in ['lib.rs','main.rs','BaseEntity.rs','Cargo.toml','Cargo.lock','build.rs']:
            files[name]=hashlib.sha256((ROOT/name).read_bytes()).hexdigest()
        fingerprint=hashlib.sha256(json.dumps(files,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        result['source_fingerprint']=fingerprint
        (output/'source-manifest.json').write_text(json.dumps({'fingerprint':fingerprint,'files':files},indent=2)+'\n')
        sdk=setup()
        result['graphics_sdk']=sdk
        execute(['cargo','build','--offline','--locked','--lib','--target','aarch64-apple-ios'],output/'rust-build.log',300)
        assets=output/'EngineAssets';assets.mkdir(exist_ok=True)
        for directory in ['Procedures','Systems']:
            shutil.copytree(ROOT/directory,assets/directory,dirs_exist_ok=True)
        (assets/'BaseSystem').mkdir(exist_ok=True)
        for name in ['registry.json','circuit_breaker.json']:shutil.copy2(ROOT/'BaseSystem'/name,assets/'BaseSystem'/name)
        (assets/'source-fingerprint.json').write_text(json.dumps({'fingerprint':fingerprint})+'\n')
        notices=assets/'ThirdPartyLicenses'
        shutil.copytree(ROOT/'BuildSupport/Licenses',notices/'common',dirs_exist_ok=True)
        for name in ['LICENSE','THIRD_PARTY_NOTICES.md']:shutil.copy2(ROOT/name,notices/name)
        for component in ['wgpu','moltenvk']:
            sdk_root=ROOT/'output/ios-sdk'/component
            for path in sdk_root.rglob('*'):
                if path.is_file() and any(word in path.name.lower() for word in ['license','licensing','copyright','notice']):
                    destination=notices/component/path.relative_to(sdk_root)
                    destination.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(path,destination)

        project=output/'project'
        execute(['cmake','-S',ROOT/'BuildSupport/iOS','-B',project,'-G','Xcode','-DCMAKE_SYSTEM_NAME=iOS','-DCMAKE_OSX_SYSROOT=iphoneos','-DCMAKE_OSX_ARCHITECTURES=arm64','-DCMAKE_OSX_DEPLOYMENT_TARGET=16.0',f'-DWORMWOOD_TEAM={team}',f'-DWORMWOOD_ASSETS={assets}',f'-DWORMWOOD_RUST_LIBRARY={ROOT}/target/aarch64-apple-ios/debug/libwormwood.a',f"-DWORMWOOD_WGPU_LIBRARY={sdk['wgpu_library']}",f"-DWORMWOOD_MOLTENVK_LIBRARY={sdk['moltenvk_library']}",f"-DWORMWOOD_WGPU_INCLUDE={sdk['wgpu_include']}"],output/'configure.log')
        selected=None
        if device:
            details=output/'devices.json'
            execute(['xcrun','devicectl','list','devices','--timeout','10','--json-output',details])
            devices=json.loads(details.read_text())['result']['devices']
            selected=next(d for d in devices if d.get('identifier')==device or d.get('hardwareProperties',{}).get('udid')==device)
        destination=f"id={selected['hardwareProperties']['udid']}" if selected else 'generic/platform=iOS'

        execute(['xcodebuild','-project',project/'WormwoodIOS.xcodeproj','-scheme','Wormwood','-configuration','Debug','-destination',destination,'-jobs','2','-allowProvisioningUpdates','-allowProvisioningDeviceRegistration','build'],output/'xcode-build.log',args.signing_timeout)
        app=project/'Debug-iphoneos/Wormwood.app'
        execute(['codesign','--verify','--strict',app],output/'codesign-verify.log')
        profile=plistlib.loads(subprocess.check_output(['security','cms','-D','-i',str(app/'embedded.mobileprovision')]))
        if team not in profile.get('TeamIdentifier',[]):raise RuntimeError('Provisioning profile team mismatch')
        if selected and selected['hardwareProperties']['udid'] not in profile.get('ProvisionedDevices',[]):raise RuntimeError('Profile does not include selected phone')
        for name,expected in files.items():
            if hashlib.sha256((ROOT/name).read_bytes()).hexdigest()!=expected:raise RuntimeError(f'Source changed during build: {name}')
        result['signed_build_verified']=True
        if args.build_only:return
        execute(['xcrun','devicectl','device','install','app','--device',device,str(app),'--timeout','120','--json-output',output/'install.json'],output/'install.log')
        result['deployed']=True
        execute(['xcrun','devicectl','device','process','launch','--device',device,'--terminate-existing','--timeout','30','--json-output',output/'launch.json','org.salamander.wormwood'],output/'launch.log')
        # Fetch app-produced evidence, not merely the launch command's success.
        report=output/'host-bootstrap.json'
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            report.unlink(missing_ok=True)
            fetched=subprocess.run(['xcrun','devicectl','device','copy','from','--device',device,'--domain-type','appDataContainer','--domain-identifier','org.salamander.wormwood','--source','Documents/Wormwood/host-bootstrap.json','--destination',str(report),'--timeout','5'],cwd=ROOT,capture_output=True,text=True,timeout=10)
            if fetched.returncode==0 and report.exists():
                actual=json.loads(report.read_text())
                if actual.get('source_fingerprint')==fingerprint:
                    if not actual.get('runtime_validated'):raise RuntimeError(f'Rust bootstrap failed on phone: {actual}')
                    result['host_report']=actual;break
            time.sleep(0.5)
        else:raise RuntimeError('No matching on-device Rust bootstrap report received')
        execute(['xcrun','devicectl','device','copy','from','--device',device,'--domain-type','appDataContainer','--domain-identifier','org.salamander.wormwood','--source','Documents/Wormwood/execution.jsonl','--destination',output/'execution.jsonl','--timeout','10'],output/'trace-copy.log')
        rows=[json.loads(line) for line in (output/'execution.jsonl').read_text().splitlines()]
        assert any(row.get('event')=='finished' and row.get('function')=='DiagnosticsSystem.ValidateRuntime' for row in rows)
        result['rust_host_verified_on_device']=True
        cues_path=output/'ios-cues.json'
        deadline=time.monotonic()+35
        while time.monotonic()<deadline:
            cues_path.unlink(missing_ok=True)
            fetched=subprocess.run(['xcrun','devicectl','device','copy','from','--device',device,'--domain-type','appDataContainer','--domain-identifier','org.salamander.wormwood','--source','Documents/Wormwood/ios-cues.json','--destination',str(cues_path),'--timeout','5'],cwd=ROOT,capture_output=True,text=True,timeout=10)
            if fetched.returncode==0 and cues_path.exists():
                cues=json.loads(cues_path.read_text())
                if cues.get('source_fingerprint')==fingerprint:break
            time.sleep(.5)
        else:raise RuntimeError('No matching on-device native cue report received')
        result['native_cues']=cues
        # Copy the final trace after cue execution, not only the bootstrap prefix.
        execute(['xcrun','devicectl','device','copy','from','--device',device,'--domain-type','appDataContainer','--domain-identifier','org.salamander.wormwood','--source','Documents/Wormwood/execution.jsonl','--destination',output/'execution.jsonl','--timeout','10'],output/'trace-copy.log')
        rows=[json.loads(line) for line in (output/'execution.jsonl').read_text().splitlines()]
        rows=[row for row in rows if row.get('run_id')==cues['execution_run_id']]
        if not rows:raise RuntimeError('No execution trace matches the native cue run')
        passed=[v for v in cues['backends'] if v['passed']]
        for value in passed:
            report=value['report'];audio=report['audio']
            assert report['stage']=='Finished' and report['entities_remaining']==0
            assert report['presentation']['frames']==60 and report['presentation']['device']['readback_verified']
            assert audio['completed'] and audio['output_buffer_verified'] and audio['active_callback_discontinuities']==0
            assert audio['samples_produced']==int(audio['sample_rate_hz']*.75)
            events=[row for row in rows if row.get('audicle')==report['audicle']]
            assert [row['stage'] for row in events if row['event']=='audicle_stage']==['Preparing','Ready','Go','Finished']
            assert sum(row['event']=='frame_presented' for row in events)==60
            assert sum(row['event']=='entity_created' for row in events)==2
            assert sum(row['event']=='entity_removed' for row in events)==2
            assert not any(row['event'] in ['failed','rejected','audicle_cancelled'] for row in events)
            for event in ['audio_prepared','audio_started','audio_completed','audio_released']:
                assert sum(row['event']==event for row in events)==1
        result['verified_backends']=[v['backend'] for v in passed]
        result['graphics_verified']=cues['passed'];result['audio_verified']=bool(passed)
        print('iPhone native cue results: '+', '.join(result['verified_backends']),flush=True)
        if not cues['passed']:raise RuntimeError('Native cue verification incomplete; inspect ios-cues.json for individual API results')
    except Exception as error:
        result['error']=str(error);raise
    finally:
        result_path.write_text(json.dumps(result,indent=2)+'\n')

if __name__=='__main__':main()
