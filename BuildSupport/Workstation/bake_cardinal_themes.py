#!/usr/bin/env python3
"""Explicit OFFLINE build of Cardinal's original crystal shader into 2D theme artwork.
Never invoked by startup, theme selection, UI interaction, or an audio callback.
The 3D gem ruby remains Wormwood's main theme. This is alternative 2D artwork.
"""
import argparse, array, hashlib, json, math, pathlib, subprocess, tempfile
ROOT=pathlib.Path(__file__).resolve().parents[2]
THEMES=ROOT/'Art/AudioWorkstation/CardinalThemes'
EDGE=256
TARGETS=[('panel','panel_top'),('panel','panel_bottom'),('panel','panel_left'),('panel','panel_right'),('button','transport_rewind'),('button','transport_stop'),('button','transport_record'),('button','transport_play'),('button','*'),('surface','fader_housing_default'),('surface','fader_knob_default'),('surface','timeline'),('surface','track_default:audio'),('surface','clip_default:midi')]
def material(d,kind,key):
 field='panel_material' if kind=='panel' else 'button_material' if kind=='button' else 'fader_material' if key.startswith('fader_') else 'track_handle_material' if key.startswith('track_') else 'timeline_material'
 return d.get(field,'original')
def color(d,kind,key):
 default=d.get('panel' if kind=='panel' else 'button','E6E0D4FF')
 if kind=='panel':c=d.get(key,default)
 elif kind=='button':c=d.get('button_colors',{}).get(key,d.get(key+'_button',default))
 else:c=d.get('surface_colors',{}).get(key,d.get('lane',default) if key.startswith('clip_') else default)
 return [int(c[i:i+2],16)/255 for i in (0,2,4)]
def bake(path,probe,out):
 d=json.loads(path.read_text()); entries=[]; rows=[]
 for kind,key in TARGETS:
  if material(d,kind,key)!='gem':continue
  settings=dict(d.get('gem_material',{}));settings.update(d.get('panel_gems',{}).get(key,{}) if kind=='panel' else {})
  for aspect in (.25,1.,4.):
   tint=color(d,kind,key);sigma=[.018]*3 if settings.get('clear',False) else [-math.log(min(.98,max(.015,c)))*1.7+.03 for c in tint]
   unit=min(aspect,1);az=math.radians(settings.get('azimuth',315));el=math.radians(settings.get('elevation',48))
   # Same material/absorption/tilt uniforms as Cardinal RenderGroup. Lighting
   # is captured once at the origin; runtime placement never relights a panel.
   data=[[aspect/unit,1/unit,settings.get('depth',.85),.31 if settings.get('split_cut',True) else .19],
    [1.766,settings.get('absorption',1.15),.006 if settings.get('dispersion',False) else 0,settings.get('exposure',1.1)],
    [0,0,settings.get('tilt_x',.12),settings.get('tilt_y',-.12)],
    [float(settings.get('facets',False)),float(settings.get('split_cut',True)),1,1],
    sigma+[1],[math.cos(az)*math.cos(el),math.sin(az)*math.cos(el),math.sin(el),0]]
   rows.extend(data);entries.append(dict(key=kind+':'+key,aspect=aspect))
 if not entries:return
 width=8*EDGE;height=math.ceil(len(entries)/8)*EDGE
 source=(THEMES/'Reference/DawGem.frag.wgsl').read_text()
 source=source.replace('@group(0) @binding(0) var<uniform> u:Uniforms;','var<private> u:Uniforms;\nstruct Data {rows:array<vec4<f32>>};\n@group(0) @binding(1) var<storage,read> data:Data;')
 source=source.replace('@fragment fn fs_main','fn cardinalGem').replace('{discard;}','{return vec4<f32>(0.0);}')
 source=source.replace('->@location(0) vec4<f32>','->vec4<f32>')
 source+='''
fn workstationShade(screen:vec2<f32>)->vec4<f32>{
 let pixel=vec2<u32>(u32((screen.x+1.0)*0.5*WIDTH),u32((1.0-screen.y)*0.5*HEIGHT));
 let tile=pixel/EDGEu;let item=tile.y*8u+tile.x;if(item>=COUNTu){return vec4<f32>(0.0);}
 let at=256u+item*6u;u.model=mat4x4<f32>(data.rows[at],data.rows[at+1u],data.rows[at+2u],data.rows[at+3u]);u.color=data.rows[at+4u];u.lightAndGrid=data.rows[at+5u];
 return cardinalGem(FSIn((vec2<f32>(pixel%EDGEu)+0.5)/EDGE.0));
}
'''.replace('WIDTH',str(float(width))).replace('HEIGHT',str(float(height))).replace('COUNT',str(len(entries))).replace('EDGE',str(EDGE))
 dest=out/path.stem;dest.mkdir(parents=True,exist_ok=True)
 with tempfile.TemporaryDirectory(prefix='cardinal-theme-') as tmp:
  tmp=pathlib.Path(tmp);shader=tmp/'theme.wgsl';shader.write_text(source);data=tmp/'data.bin';a=array.array('f',[0.]*(256*4)+[v for row in rows for v in row]);a[32:34]=array.array('f',[width,height]);data.write_bytes(a.tobytes());prefix=tmp/'bake'
  result=subprocess.run([str(probe),str(shader),str(data),str(width),str(height),str(prefix),'1'],capture_output=True,text=True)
  if result.returncode:raise RuntimeError(result.stdout+'\n'+result.stderr)
  linear=array.array('f');linear.frombytes(prefix.with_suffix('.rgba32f').read_bytes())
  packed=bytes(round(max(0.,min(1.,v))*255) for v in linear)
  (dest/'images.rgba8').write_bytes(packed)
  (dest/'images.rgba32f').unlink(missing_ok=True)
 for i,e in enumerate(entries):e.update(offset=(i//8)*EDGE*width+(i%8)*EDGE,edge=EDGE,stride=width)
 (dest/'manifest.json').write_text(json.dumps(dict(schema=1,theme=d,shader_sha256=hashlib.sha256((THEMES/'Reference/DawGem.frag.wgsl').read_bytes()).hexdigest(),entries=entries),indent=2)+'\n')
 print(path.stem,len(entries),'crystal images',flush=True)
def bake_grain(temp):
 source=(THEMES/'Reference/DawGrainTexture.h').read_text()
 helpers=source[source.index('constexpr int kTextureSize'):source.index('inline bool Ensure')]
 body=source[source.index('    std::vector<unsigned char> pixels'):source.index('    TextureUploadParams params;')]
 cpp=temp/'grain.cpp';binary=temp/'grain'
 cpp.write_text('#include <algorithm>\n#include <cmath>\n#include <cstddef>\n#include <vector>\n#include <fstream>\n'+helpers+'\nint main(int argc,char**argv){\n'+body+'\nstd::ofstream f(argv[1],std::ios::binary);f.write(reinterpret_cast<const char*>(pixels.data()),pixels.size());return f?0:1;}')
 subprocess.run(['xcrun','clang++','-O2','-std=c++17',str(cpp),'-o',str(binary)],check=True)
 subprocess.run([str(binary),str(THEMES/'grain.rgba8')],check=True)

def main():
 ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('themes',nargs='*',type=pathlib.Path);ap.add_argument('--probe',type=pathlib.Path);ap.add_argument('--output',type=pathlib.Path,default=THEMES/'Baked');args=ap.parse_args()
 with tempfile.TemporaryDirectory(prefix='cardinal-baker-') as temp:
  bake_grain(pathlib.Path(temp))
  probe=args.probe
  if not probe:
   probe=pathlib.Path(temp)/'compute-probe'
   subprocess.run(['xcrun','clang++','-std=c++17','-O2','-I/opt/homebrew/include',str(ROOT/'BuildSupport/Workstation/compute_probe.cpp'),'-L/opt/homebrew/lib','-lwgpu_native','-Wl,-rpath,/opt/homebrew/lib','-o',str(probe)],check=True)
  for path in args.themes or sorted(THEMES.glob('*.json')):bake(path,probe,args.output)
if __name__=='__main__':main()
