#!/usr/bin/env python3
"""Native persistent-window check using explicitly labelled diagnostic commands."""
import json, os, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def main():
    out=ROOT/'output'; saved=out/'interactive-verification'; saved.mkdir(parents=True,exist_ok=True)
    summary={'passed':False,'backends':[],'input_source':'diagnostic_commands','limits':'Native drawing and JACK playback; physical keyboard input is not synthesized. Preparation and native teardown remain synchronous.'}
    try:
        for backend in ['OpenGL','Vulkan','WebGPU']:
            trace=out/'execution.jsonl';offset=trace.stat().st_size if trace.exists() else 0
            report_path=out/f'interactive-{backend}.json';report_path.unlink(missing_ok=True)
            result=subprocess.run([str(ROOT/'target/debug/wormwood'),'--interactive','hello_triangle',backend,'--verify-session'],cwd=ROOT,capture_output=True,text=True,timeout=40)
            (saved/f'{backend}.stdout.log').write_text(result.stdout);(saved/f'{backend}.stderr.log').write_text(result.stderr)
            assert result.returncode==0,result.stderr
            report=json.loads(report_path.read_text());assert report['cues_finished']==2 and report['cues_cancelled']==1 and report['entities_remaining']==0,report
            with trace.open('rb') as f:f.seek(offset); rows=[json.loads(line) for line in f]
            assert [r['state'] for r in rows if r['event']=='host_startup']==['Starting','Started']
            for event,count in [('native_prepared',1),('native_released',1),('audio_prepared',3),('audio_started',3),('audio_completed',2),('audio_released',3),('audicle_cancelled',1),('entity_created',4),('entity_removed',4)]:
                assert sum(r['event']==event for r in rows)==count,(event,count)
            completed=[r for r in rows if r['event']=='audio_completed'];assert all(r['report']['active_xruns']==0 and r['report']['loopback_verified'] for r in completed)
            assert not any(r['event'] in ['failed','rejected'] for r in rows)
            pumps=[r for r in rows if r['event']=='window_pumped'];assert pumps and all(r['readback_verified'] for r in pumps)
            # Drawing persists in the idle interval after Finish/Cancel.
            assert any(r['audicle'] is None and r['drawn'] for r in pumps)
            (saved/f'{backend}.json').write_text(json.dumps(report,indent=2)+'\n')
            (saved/f'{backend}.trace.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
            summary['backends'].append({'backend':backend,**report})
            print(f'{backend}: one window, two finishes, one cancellation, three audio starts, zero active xruns, complete cleanup',flush=True)
        # A real native graphics preparation followed by missing JACK must unwind.
        offset=trace.stat().st_size
        result=subprocess.run([str(ROOT/'target/debug/wormwood'),'--interactive','hello_triangle','OpenGL','--verify-session'],cwd=ROOT,env={**os.environ,'WORMWOOD_JACK_LIBRARY':'/nonexistent/wormwood-jack.dylib'},capture_output=True,text=True,timeout=20)
        assert result.returncode!=0
        with trace.open('rb') as f:f.seek(offset);rows=[json.loads(line) for line in f]
        assert sum(r['event']=='audicle_cancelled' for r in rows)==1
        assert sum(r['event']=='entity_created' for r in rows)==sum(r['event']=='entity_removed' for r in rows)==2
        assert sum(r['event']=='native_released' for r in rows)==1
        (saved/'missing-jack.trace.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
        summary['missing_jack_cleanup']=True;summary['passed']=True
    except Exception as error:summary['error']=str(error);raise
    finally:(saved/'verification.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
