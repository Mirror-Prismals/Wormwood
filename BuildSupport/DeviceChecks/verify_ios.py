#!/usr/bin/env python3
"""Launch and verify the already-installed source-fingerprinted iPhone build."""
from pathlib import Path
import argparse,json,subprocess,time
ROOT=Path(__file__).resolve().parents[2]

def copy_file(device,source,destination):
    return subprocess.run(['xcrun','devicectl','device','copy','from','--device',device,'--domain-type','appDataContainer','--domain-identifier','org.salamander.wormwood','--source','Documents/Wormwood/'+source,'--destination',str(destination),'--timeout','5'],capture_output=True,text=True,timeout=10)

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--device');args=parser.parse_args()
    config=json.loads((ROOT/'output/ios-signing.json').read_text());device=args.device or config['device_identifier']
    out=ROOT/'output/ios-device-build';path=out/'verification.json';result=json.loads(path.read_text())
    if not result['signed_build_verified'] or not result['deployed']:raise RuntimeError('Requires a verified signed and installed build')
    fingerprint=result['source_fingerprint']
    app=out/'project/Debug-iphoneos/Wormwood.app'
    packaged=json.loads((app/'EngineAssets/source-fingerprint.json').read_text())
    if packaged['fingerprint']!=fingerprint:raise RuntimeError('Signed bundle does not match the installed build record')
    subprocess.run(['codesign','--verify','--strict',str(app)],check=True)
    result.update(rust_host_verified_on_device=False,graphics_verified=False,audio_verified=False)
    result.pop('error',None)
    try:
        launch=subprocess.run(['xcrun','devicectl','device','process','launch','--device',device,'--terminate-existing','--timeout','15','--json-output',str(out/'launch.json'),'org.salamander.wormwood'],capture_output=True,text=True,timeout=25)
        (out/'launch.log').write_text(launch.stdout+launch.stderr)
        if launch.returncode:raise RuntimeError('Phone launch failed; see launch.log (unlock the phone if Locked)')
        deadline=time.monotonic()+40
        while time.monotonic()<deadline:
            destination=out/'ios-cues.json';destination.unlink(missing_ok=True)
            fetched=copy_file(device,'ios-cues.json',destination)
            if fetched.returncode==0 and destination.exists():
                cues=json.loads(destination.read_text())
                if cues.get('source_fingerprint')==fingerprint:break
            time.sleep(.5)
        else:raise RuntimeError('No source-matched native cue report returned; inspect app crash/device logs')
        result['native_cues']=cues
        for filename in ['host-bootstrap.json','execution.jsonl']:
            fetched=copy_file(device,filename,out/filename)
            if fetched.returncode:raise RuntimeError(f'Cannot retrieve {filename}: {fetched.stderr}')
        bootstrap=json.loads((out/'host-bootstrap.json').read_text())
        assert bootstrap['source_fingerprint']==fingerprint and bootstrap['runtime_validated']
        result['rust_host_verified_on_device']=True
        rows=[json.loads(line) for line in (out/'execution.jsonl').read_text().splitlines()]
        rows=[row for row in rows if row.get('run_id')==cues['execution_run_id']];assert rows
        passed=[]
        for value in cues['backends']:
            print(value['backend'], 'passed' if value['passed'] else value['error'],flush=True)
            if not value['passed']:continue
            report=value['report'];audio=report['audio'];presentation=report['presentation']
            assert report['stage']=='Finished' and report['entities_remaining']==0
            assert presentation['frames']==60 and presentation['device']['readback_verified']
            assert audio['completed'] and audio['output_buffer_verified'] and audio['active_callback_discontinuities']==0
            assert audio['samples_produced']==int(audio['sample_rate_hz']*.75)
            events=[row for row in rows if row.get('audicle')==report['audicle']]
            assert [row['stage'] for row in events if row['event']=='audicle_stage']==['Preparing','Ready','Go','Finished']
            for kind,count in [('frame_presented',60),('entity_created',2),('entity_removed',2),('audio_prepared',1),('audio_started',1),('audio_completed',1),('audio_released',1)]:assert sum(row['event']==kind for row in events)==count,kind
            assert not any(row['event'] in ['failed','rejected','audicle_cancelled'] for row in events)
            passed.append(value['backend'])
        result['verified_backends']=passed
        result['graphics_verified']=cues['passed'];result['audio_verified']=bool(passed)
        if not cues['passed']:raise RuntimeError('One or more explicit iOS API paths failed; see ios-cues.json')
    except Exception as error:result['error']=str(error);raise
    finally:path.write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
