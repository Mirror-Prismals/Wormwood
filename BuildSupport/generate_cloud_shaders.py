"""Embed canonical cloud equations and backend resource access in scene shaders.
Regenerate in this order: sky, woodland, clouds. No shader compiler is invoked.
"""
from pathlib import Path
import re
ROOT=Path(__file__).resolve().parents[1]
material_bytes=(ROOT/'Procedures/Assets/Woodland/materials.rgba').stat().st_size
noise_bytes=(ROOT/'Procedures/Assets/Clouds/noise.rgba').stat().st_size
noise_offset=16777224+material_bytes
noise_pixel=noise_offset//4
frame_offset=(noise_offset+noise_bytes+15)//16*16
frame_word=(frame_offset-8)//4
src=(ROOT/'Procedures/Shaders/clouds.shared.glsl').read_text()
START='// BEGIN GENERATED CLOUDS\n';END='// END GENERATED CLOUDS\n'
def wgsl(source):
 types={'float':'f32','int':'i32','bool':'bool','vec2':'vec2<f32>','vec3':'vec3<f32>','vec4':'vec4<f32>','ivec2':'vec2<i32>'}
 def fn(m):
  ret,name,args=m.groups();params=[]
  for arg in args.split(','):
   t,n=arg.strip().split();params.append(n+':'+types[t])
  return 'fn '+name+'('+','.join(params)+')->'+types[ret]+' {'
 source=re.sub(r'\b(float|int|bool|vec2|vec3|vec4|ivec2) (\w+)\(([^()]*)\)\s*\{',fn,source)
 source=re.sub(r'\b(float|int|bool|vec2|vec3|vec4|ivec2) (\w+)\s*=',lambda m:'var '+m[2]+':'+types[m[1]]+'=',source)
 for t in ['ivec2','vec4','vec3','vec2','float','int']:
  source=re.sub(r'\b'+t+r'\(',types[t]+'(',source)
 # WGSL has no vector +/- scalar overloads (multiply/divide are supported).
 source=source.replace('point-0.5','point-vec2<f32>(0.5)').replace('warp.gba-0.5','warp.gba-vec3<f32>(0.5)')
 # WGSL requires compound blocks even for single-statement if bodies.
 edits=[]
 for m in re.finditer(r'\bif\s*\(',source):
  a=source.index('(',m.start());depth=1;b=a+1
  while depth:
   if source[b]=='(':depth+=1
   elif source[b]==')':depth-=1
   b+=1
  start=b
  while source[start].isspace():start+=1
  if source[start]!='{':
   end=source.index(';',start)+1;edits.append((start,end))
 for start,end in reversed(edits):source=source[:start]+'{'+source[start:end]+'}'+source[end:]
 source=source.replace('64>>level','64>>u32(level)').replace('1<<layer','1<<u32(layer)').replace('1<<chosen','1<<u32(chosen)')
 return source
for name in ['triangle.gl.frag','triangle.frag','scene.ios.frag','triangle.wgsl']:
 p=ROOT/'Procedures/Shaders'/name;s=p.read_text();wg=name.endswith('wgsl');vk=name=='triangle.frag'
 if START in s:
  a=s.index(START);b=s.index(END,a)+len(END);s=s[:a]+s[b:]
 if wg:
  access=f'''fn cloudNoiseFetch(point:vec2<i32>)->vec4<f32>{{let p=clamp(point,vec2<i32>(0),vec2<i32>(527,722));return unpack4x8unorm(playerTexture.pixels[{noise_pixel}u+u32(p.y*528+p.x)]);}}
fn cloudRow(i:i32)->vec4<f32>{{let k={frame_word}u+u32(i)*4u;return vec4<f32>(bitcast<f32>(playerTexture.pixels[k]),bitcast<f32>(playerTexture.pixels[k+1u]),bitcast<f32>(playerTexture.pixels[k+2u]),bitcast<f32>(playerTexture.pixels[k+3u]));}}
'''
  code=access+wgsl(src);at=s.index('@fragment fn fs_main')
  s=s.replace('return expanseSky(i.data.xyz,i.material);','return cloudComposite(i.data.xyz,expanseSky(i.data.xyz,i.material));')
 else:
  if vk:
   access=f'''vec4 cloudNoiseFetch(ivec2 point){{ivec2 p=clamp(point,ivec2(0),ivec2(527,722));return unpackUnorm4x8(playerTexture.pixels[{noise_pixel}u+uint(p.y*528+p.x)]);}}
vec4 cloudRow(int i){{uint k={frame_word}u+uint(i)*4u;return vec4(uintBitsToFloat(playerTexture.pixels[k]),uintBitsToFloat(playerTexture.pixels[k+1u]),uintBitsToFloat(playerTexture.pixels[k+2u]),uintBitsToFloat(playerTexture.pixels[k+3u]));}}
'''
  else:
   access='uniform sampler2D cloudNoiseAtlas;uniform vec4 cloudFrame[12];\nvec4 cloudNoiseFetch(ivec2 p){return texelFetch(cloudNoiseAtlas,clamp(p,ivec2(0),ivec2(527,722)),0);}\nvec4 cloudRow(int i){return cloudFrame[i];}\n'
  code=access+src;at=s.index('void main()')
  s=s.replace('pixel=expanseSky(data.xyz,material);','pixel=cloudComposite(data.xyz,expanseSky(data.xyz,material));')
 # Re-find after replacing the call, since it can change source length.
 at=s.index('@fragment fn fs_main') if wg else s.index('void main()')
 s=s[:at]+START+code+END+s[at:];p.write_text(s)
