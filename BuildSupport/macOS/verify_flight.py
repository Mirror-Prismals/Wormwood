#!/usr/bin/env python3
import json,subprocess,shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def main():
 out=ROOT/'output';saved=out/'flight-verification';saved.mkdir(parents=True,exist_ok=True);summary={'passed':False,'backends':[],'input_source':'diagnostic_commands'}
 try:
  for backend in ['OpenGL','Vulkan','WebGPU']:
   trace=out/'execution.jsonl';offset=trace.stat().st_size if trace.exists() else 0
   result=subprocess.run([str(ROOT/'target/debug/wormwood'),'--interactive','hello_cube',backend,'--verify-session'],cwd=ROOT,capture_output=True,text=True,timeout=25)
   (saved/f'{backend}.stderr.log').write_text(result.stderr);assert result.returncode==0,result.stderr
   report=json.loads((out/f'interactive-{backend}.json').read_text());assert report['entities_remaining']==0 and report['frames']==72 and report['flight']['position']==[3.,2.,5.] and not report['flight']['moving'],report
   with trace.open('rb') as f:f.seek(offset);rows=[json.loads(line) for line in f]
   assert not any(r['event'] in ['failed','rejected'] for r in rows)
   for event,count in [('entity_created',1),('entity_removed',1),('native_prepared',1),('native_released',1),('window_pumped',72)]:assert sum(r['event']==event for r in rows)==count,event
   steps=[r for r in rows if r['event']=='flight_step'];assert any(r['position']!=[3.,2.,5.] for r in steps);assert len(steps)>30
   assert all(r['audicle'] is not None for r in steps)
   assert all(r['readback_verified'] for r in rows if r['event']=='window_pumped')
   assert sum(r['stage']=='Preparing' for r in rows if r['event']=='audicle_stage')==sum(r['stage']=='Finished' for r in rows if r['event']=='audicle_stage')
   (saved/f'{backend}.json').write_text(json.dumps(report,indent=2)+'\n');(saved/f'{backend}.trace.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in rows))
   shutil.copy2(out/f'triangle-{backend}.ppm',saved/f'cube-{backend}.ppm')
   summary['backends'].append({'backend':backend,**report});print(f'{backend}: cube readback, flight/look/reset, Audicle attribution and cleanup passed',flush=True)
  summary['passed']=True
 except Exception as error:summary['error']=str(error);raise
 finally:(saved/'verification.json').write_text(json.dumps(summary,indent=2)+'\n')
if __name__=='__main__':main()
