#!/usr/bin/env python3
"""Verify native Mac close-request cancellation; this does not simulate a mouse click."""
from pathlib import Path
import json,subprocess
ROOT=Path(__file__).resolve().parents[2]
def main():
    out=ROOT/'output';saved=out/'live-verification';saved.mkdir(parents=True,exist_ok=True)
    summary={'passed':False,'platform':'macOS','close_source':'diagnostic_request','backends':[],
        'limits':'Real native API close flag and cleanup; a human window-button click was not automated. The GUI automation service timed out. Audio is released on cancellation, not reported as completed.'}
    try:
        for backend in ['OpenGL','Vulkan','WebGPU']:
            trace=out/'execution.jsonl';offset=trace.stat().st_size if trace.exists() else 0
            path=out/f'live-{backend}.json';path.unlink(missing_ok=True)
            run=subprocess.run([str(ROOT/'target/debug/wormwood'),'--live','hello_triangle',backend,'--close-after-frames','12'],cwd=ROOT,capture_output=True,text=True,timeout=20)
            (saved/f'{backend}.stdout.log').write_text(run.stdout);(saved/f'{backend}.stderr.log').write_text(run.stderr)
            if run.returncode:raise RuntimeError(f'{backend}: {run.stderr}')
            report=json.loads(path.read_text());presentation=report['presentation']
            assert report['stage']=='Cancelled' and report['reason']=='window_closed' and report['entities_remaining']==0
            assert presentation['backend']==backend and presentation['frames']==12 and presentation['close_source']=='diagnostic_request' and presentation['device']['readback_verified']
            assert report['audio'] is None
            with trace.open('rb') as f:f.seek(offset);rows=[json.loads(line) for line in f]
            assert [r['state'] for r in rows if r['event']=='host_startup']==['Starting','Started']
            assert [r['stage'] for r in rows if r['event']=='audicle_stage']==['Preparing','Ready','Go']
            for event,count in [('audicle_cancelled',1),('entity_created',2),('entity_removed',2),('native_released',1),('audio_started',1),('audio_released',1),('audio_completed',0),('frame_presented',12)]:assert sum(r['event']==event for r in rows)==count,event
            assert not any(r['event'] in ['failed','rejected'] for r in rows)
            prepared=next(r for r in rows if r['event']=='audio_prepared')
            server_log=(out/'jack-server.log').read_text()
            if prepared['configuration']['owned_server']:assert 'caught signal 15' in server_log
            (saved/f'{backend}.server.log').write_text(server_log)
            (saved/f'{backend}.json').write_text(json.dumps(report,indent=2)+'\n')
            (saved/f'{backend}.trace.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
            summary['backends'].append(report)
            print(f'{backend}: native close requested after 12 frames; cancellation, audio/client/server, and Entity cleanup passed',flush=True)
        summary['passed']=True
    except Exception as error:summary['error']=str(error);raise
    finally:(saved/'verification.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
