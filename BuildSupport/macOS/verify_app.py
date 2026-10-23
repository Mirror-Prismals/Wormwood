#!/usr/bin/env python3
"""Relocate the app and verify packaged assets and native API paths."""
from pathlib import Path
import hashlib,json,shutil,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
def hashes(app):return {str(p.relative_to(app)):hashlib.sha256(p.read_bytes()).hexdigest() for p in app.rglob('*') if p.is_file()}
def main():
    project=json.loads((ROOT/'Project.json').read_text())
    source=ROOT/'output/macOS'/f"{project['name']}.app";saved=ROOT/'output/macOS/app-verification';saved.mkdir(exist_ok=True)
    output=Path.home()/'Library/Application Support'/project['data_directory']/'Diagnostics'
    summary={'passed':False,'backends':[],'bundle_resources_unchanged':False}
    try:
        with tempfile.TemporaryDirectory(prefix='Wormwood app verification ') as tmp:
            app=Path(tmp)/'Wormwood.app';shutil.copytree(source,app);before=hashes(app)
            for backend in ['OpenGL','Vulkan','WebGPU']:
                report=output/f'interactive-{backend}.json';report.unlink(missing_ok=True)
                trace=output/'execution.jsonl';offset=trace.stat().st_size if trace.exists() else 0
                run=subprocess.run([str(app/'Contents/MacOS/Wormwood'),'--verify-bundle',backend],cwd=tmp,capture_output=True,text=True,timeout=40)
                (saved/f'{backend}.stdout.log').write_text(run.stdout);(saved/f'{backend}.stderr.log').write_text(run.stderr)
                assert run.returncode==0,run.stderr
                result=json.loads(report.read_text());assert result['cues_finished']==2 and result['cues_cancelled']==1 and result['entities_remaining']==0,result
                with trace.open('rb') as f:f.seek(offset);rows=[json.loads(line) for line in f]
                prepared=[row['configuration'] for row in rows if row['event']=='audio_prepared']
                assert len(prepared)==3 and all(c['owned_server'] and c['follows_system_default'] and c['output_device_uid'] for c in prepared)
                completed=[row['report'] for row in rows if row['event']=='audio_completed']
                assert len(completed)==2 and all(c['active_xruns']==0 and c['loopback_verified'] for c in completed)
                (saved/f'{backend}.trace.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
                (saved/f'{backend}.json').write_text(json.dumps(result,indent=2)+'\n')
                summary['backends'].append({'backend':backend,**result})
                print(f'{backend}: relocated bundle passed',flush=True)
            assert before==hashes(app),'Bundle modified by runtime'
            subprocess.run(['codesign','--verify','--strict',str(app)],check=True)
            summary['bundle_resources_unchanged']=True;summary['passed']=True
    except Exception as error:summary['error']=str(error);raise
    finally:(saved/'verification.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
