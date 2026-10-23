#!/usr/bin/env python3
"""Finite paced same-process macOS callback/rendering and lifecycle checks."""
from pathlib import Path
import argparse,hashlib,json,subprocess,time
ROOT=Path(__file__).resolve().parents[2]
def fingerprint():
    files={}
    for directory in ['BaseSystem','Host','Render','Procedures','Systems']:
        for path in sorted((ROOT/directory).rglob('*')):
            if path.is_file():files[path.relative_to(ROOT).as_posix()]=hashlib.sha256(path.read_bytes()).hexdigest()
    for name in ['Cargo.lock','Cargo.toml','lib.rs','BaseEntity.rs','build.rs']:files[name]=hashlib.sha256((ROOT/name).read_bytes()).hexdigest()
    return hashlib.sha256(json.dumps(files,sort_keys=True).encode()).hexdigest()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rounds',type=int,default=2);parser.add_argument('--seconds',type=int,default=5)
    args=parser.parse_args()
    if not 1<=args.rounds<=20 or not 1<=args.seconds<=10:parser.error('rounds 1..20 and seconds 1..10 required')
    output=ROOT/'output';destination=output/'callback-render-soak';destination.mkdir(parents=True,exist_ok=True)
    summary={'passed':False,'platform':'macOS','source_fingerprint':fingerprint(),'runs':[],
        'limits':'Finite paced preallocated-callback test with repeated cues in one Host/process per API. Each cue plays 750 ms then processes silence while rendering continues. This is not sustained DAW synthesis/streaming, a memory-leak proof, device unplug/recovery, or acoustic verification.'}
    try:
        for backend in ['OpenGL','Vulkan','WebGPU']:
            trace=output/'execution.jsonl';offset=trace.stat().st_size if trace.exists() else 0
            report=output/f'soak-{backend}.json';report.unlink(missing_ok=True)
            start=time.monotonic()
            process=subprocess.run([str(ROOT/'target/debug/wormwood'),'--soak',backend,'--seconds',str(args.seconds),'--repeat',str(args.rounds)],cwd=ROOT,capture_output=True,text=True,timeout=args.rounds*(args.seconds+12)+10)
            duration=time.monotonic()-start
            (destination/f'{backend}.stdout.log').write_text(process.stdout);(destination/f'{backend}.stderr.log').write_text(process.stderr)
            if process.returncode:raise RuntimeError(f'{backend} failed: {process.stderr}')
            value=json.loads(report.read_text());assert value['passed'] and value['same_process'] and len(value['runs'])==args.rounds
            with trace.open('rb') as stream:stream.seek(offset);events=[json.loads(line) for line in stream]
            assert all(row['run_id']==value['execution_run_id'] for row in events)
            (destination/f'{backend}.trace.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in events))
            ids=set()
            for run in value['runs']:
                cue=run['audicle'];assert cue not in ids;ids.add(cue)
                rows=[row for row in events if row.get('audicle')==cue]
                a=run['audio'];p=run['presentation']
                assert run['stage']=='Finished' and run['entities_remaining']==0
                assert p['backend']==backend and p['frames']==args.seconds*60 and p['frame_period_us']==16667 and p['device']['readback_verified']
                assert a['completed'] and a['loopback_verified'] and a['samples_produced']==36000 and a['active_xruns']==0,a
                callback_seconds=a['callbacks']*a['buffer_frames']/a['sample_rate_hz'];assert callback_seconds>=args.seconds-.1,callback_seconds
                assert [row['stage'] for row in rows if row['event']=='audicle_stage']==['Preparing','Ready','Go','Finished']
                for kind,count in [('entity_created',2),('entity_removed',2),('frame_presented',args.seconds*60),('audio_prepared',1),('audio_started',1),('audio_completed',1),('audio_released',1)]:assert sum(row['event']==kind for row in rows)==count,kind
                assert not any(row['event'] in ['failed','rejected','audicle_cancelled'] for row in rows)
                summary['runs'].append({'backend':backend,'audicle':cue,'callback_seconds':callback_seconds,'report':run})
            assert duration>=args.seconds*args.rounds
            (destination/f'{backend}.json').write_text(json.dumps(value,indent=2)+'\n')
            print(f'{backend}: {args.rounds} cues in one Host/process, >= {args.seconds}s callbacks per cue, zero active xruns, cleanup passed',flush=True)
        if fingerprint()!=summary['source_fingerprint']:raise RuntimeError('Engine source changed during verification')
        summary['passed']=True
    except Exception as error:summary['error']=str(error);raise
    finally:(destination/'verification.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
