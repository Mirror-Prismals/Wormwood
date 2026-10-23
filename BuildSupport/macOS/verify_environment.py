from pathlib import Path
import json,os,subprocess
ROOT=Path(__file__).resolve().parents[2]
parent=ROOT.parent
summary={'passed':False,'games':[]}
try:
 for folder in [ROOT.name]:
  root=parent/folder;p=json.loads((root/'Project.json').read_text());app=root/'output/macOS'/f"{p['name']}.app";assets=app/'Contents/Resources/EngineAssets';saved=root/'output/environment-verification';saved.mkdir(parents=True,exist_ok=True)
  for backend in ['OpenGL','Vulkan','WebGPU']:
   out=saved/backend;out.mkdir(exist_ok=True)
   trace=out/'execution.jsonl';offset=trace.stat().st_size if trace.exists() else 0
   result=subprocess.run([str(app/'Contents/MacOS/wormwood-engine'),'--interactive',p['default_scene'],backend,'--verify-session'],cwd='/tmp',env={**os.environ,'WORMWOOD_ASSET_ROOT':str(assets),'WORMWOOD_OUTPUT_ROOT':str(out)},capture_output=True,text=True,timeout=25)
   (out/'stderr.log').write_text(result.stderr);assert result.returncode==0,result.stderr
   report=json.loads((out/f'interactive-{backend}.json').read_text());assert report['frames']==72 and report['entities_remaining']==0 and not report['flight']['moving'],report
   with trace.open('rb') as stream:stream.seek(offset);rows=[json.loads(l) for l in stream]
   assert not any(r['event'] in ['failed','rejected'] for r in rows)
   updates=[r for r in rows if r['event']=='environment_step'];assert len(updates)>=20
   assert all(r['audicle'] is not None for r in updates)
   if folder=='Wormwood':assert max(sum(x*x for x in row['state']['position'])**0.5-6371000 for row in updates)>10000
   else:assert max(abs(row['state']['up'][0]) for row in updates)>0.1
   assert all(r['readback_verified'] for r in rows if r['event']=='window_pumped')
   assert report['flight']['medium']==('planet' if folder=='Wormwood' else 'water')
   assert sum(r['event']=='entity_created' for r in rows)==sum(r['event']=='entity_removed' for r in rows)==1
   print(f"{p['display_name']}: packaged {backend} environment, movement and cleanup passed",flush=True)
  subprocess.run(['codesign','--verify','--strict',str(app)],check=True)
  summary['games'].append({'engine':p['engine'],'game':p['display_name'],'app':str(app)})
 summary['passed']=True
finally:(ROOT/'output/environment-verification/verification.json').write_text(json.dumps(summary,indent=2)+'\n')
