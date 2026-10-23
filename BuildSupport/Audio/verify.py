#!/usr/bin/env python3
"""Finite macOS combined-cue verification; plays one quiet tone per backend."""
from pathlib import Path
import json, os, subprocess
ROOT=Path(__file__).resolve().parents[2]

def run():
    output=ROOT/'output';output.mkdir(exist_ok=True)
    results=[]
    trace=output/'execution.jsonl'
    for backend in ['OpenGL','Vulkan','WebGPU']:
        offset=trace.stat().st_size if trace.exists() else 0
        report_path=output/f'av-{backend}.json'
        report_path.unlink(missing_ok=True)
        process=subprocess.run([str(ROOT/'target/debug/wormwood'),'--av',backend,'--frames','60'],cwd=ROOT,capture_output=True,text=True,timeout=20)
        (output/f'av-{backend}.stdout.log').write_text(process.stdout)
        (output/f'av-{backend}.stderr.log').write_text(process.stderr)
        assert process.returncode==0,process.stdout+process.stderr
        result=json.loads(report_path.read_text())
        audio=result['audio']
        assert result['stage']=='Finished' and result['entities_remaining']==0
        assert result['presentation']['backend']==backend and result['presentation']['frames']==60
        assert result['presentation']['device']['readback_verified']
        assert audio['loopback_verified'] and audio['completed'] and audio['samples_produced']==36000
        assert audio['active_xruns']==0,audio
        with trace.open('rb') as f:
            f.seek(offset);rows=[json.loads(line) for line in f]
        assert not any(r['event'] in ['failed','rejected','audicle_cancelled'] for r in rows)
        stages=[r['stage'] for r in rows if r['event']=='audicle_stage']
        assert stages==['Preparing','Ready','Go','Finished'],stages
        def index(event):return next(i for i,r in enumerate(rows) if r['event']==event)
        ready=next(i for i,r in enumerate(rows) if r['event']=='audicle_stage' and r['stage']=='Ready')
        go=next(i for i,r in enumerate(rows) if r['event']=='audicle_stage' and r['stage']=='Go')
        assert index('audio_prepared')<ready<go<index('audio_started')<index('frame_presented')<index('audio_completed')<index('audio_released')
        assert all(r['audicle']==result['audicle'] for r in rows if r['event'].startswith('audio_'))
        assert len([r for r in rows if r['event']=='entity_created'])==2
        assert len([r for r in rows if r['event']=='entity_removed'])==2
        serverlog=output/'jack-server.log'
        if audio['owned_server']:
            text=serverlog.read_text();assert 'caught signal 15' in text,text
            (output/f'av-{backend}.server.log').write_text(text)
        results.append(result)
    # A preparation failure must cancel, remove both Entities, and never reach Ready/Go.
    offset=trace.stat().st_size
    env=dict(os.environ,WORMWOOD_JACK_LIBRARY='/nonexistent/wormwood-test-libjack.dylib')
    failure=subprocess.run([str(ROOT/'target/debug/wormwood'),'--av','OpenGL','--frames','6'],cwd=ROOT,env=env,capture_output=True,text=True,timeout=20)
    failure_report=output/'av-OpenGL.json'
    (output/'av-failure.json').write_text(failure_report.read_text())
    failure_report.write_text(json.dumps(results[0],indent=2)+'\n')
    assert failure.returncode!=0
    (output/'av-failure.stderr.log').write_text(failure.stderr)
    with trace.open('rb') as f:
        f.seek(offset);rows=[json.loads(line) for line in f]
    assert any(r['event']=='audicle_cancelled' for r in rows)
    assert len([r for r in rows if r['event']=='entity_created'])==2
    assert len([r for r in rows if r['event']=='entity_removed'])==2
    assert not any(r['event']=='audicle_stage' and r['stage'] in ['Ready','Go','Finished'] for r in rows)
    assert not any(r['event'] in ['frame_presented','audio_started'] for r in rows)
    summary={'passed':True,'platform':'macOS','backends':results,'missing_library_cleanup_verified':True,'limits':'Finite diagnostic, not a stress or end-to-end acoustic latency test. Go coordinates submissions; no sample-accurate audiovisual synchronization is claimed.'}
    (output/'av-verification.json').write_text(json.dumps(summary,indent=2)+'\n')
    print('Combined cue verified on OpenGL, Vulkan, WebGPU; zero active xruns; preparation-failure cleanup verified.')

if __name__=='__main__':run()
