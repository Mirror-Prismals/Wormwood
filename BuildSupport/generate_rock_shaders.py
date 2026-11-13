"""Generate shared rock-map sampling for the existing scene vertex/storage ABI."""
from pathlib import Path
R=Path(__file__).resolve().parents[1]/'Procedures/Shaders'
START='// BEGIN GENERATED CAVE ROCK\n';END='// END GENERATED CAVE ROCK\n'
glsl='''vec4 cloudRow(int index);
vec4 rockTexel(int kind,int map,int level,ivec2 p){
 int size=256>>level;ivec2 xy=p & ivec2(size-1);int offset=(65536-size*size)*4/3;
 return rockRead(kind*262143+map*87381+offset+xy.y*size+xy.x);
}
vec4 rockMap(int kind,int map,vec2 uv,float lod){
 int level=int(clamp(floor(lod),0.,8.));int size=256>>level;
 vec2 p=fract(uv)*float(size)-.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 return mix(mix(rockTexel(kind,map,level,i),rockTexel(kind,map,level,i+ivec2(1,0)),f.x),mix(rockTexel(kind,map,level,i+ivec2(0,1)),rockTexel(kind,map,level,i+ivec2(1,1)),f.x),f.y);
}
vec3 rockDiffuse(vec3 rgb,vec2 metres,int tag,float footprint){
 int kind=tag/6;int face=tag%6;int axis=face/2;
 vec3 N=vec3(0);N[axis]=(face%2==0?1.:-1.);
 vec3 T=vec3(0);T[(axis+1)%3]=1.;vec3 B=vec3(0);B[(axis+2)%3]=1.;
 float lod=clamp(log2(max(footprint*128.,1.)),0.,8.);
 vec2 uv=metres/2.;vec4 a=rockMap(kind,0,uv,lod);vec4 packed=rockMap(kind,2,uv,lod);
 vec3 nm=normalize(rockMap(kind,1,uv,lod).xyz*2.-1.);
 vec3 n=normalize(T*nm.x+B*nm.y+N*nm.z);
 vec3 L=cloudRow(9).xyz;L=L/max(length(L),.0001);
 // Preserve scene lighting/fog; apply oriented normal relief relative to its
 // dominant projection plane. This is diffuse relief, not a second sun pass.
 float relief=(.30+.70*abs(dot(n,L)))/(.30+.70*abs(dot(N,L)));
 relief=mix(1.,relief,clamp(packed.r,.0,1.));
 vec3 mean=kind==0?vec3(.2836261,.20529686,.12704249):vec3(.09831836,.15777506,.2661084);
 return rgb*(a.rgb/mean)*packed.a*clamp(relief,.55,1.55);
}
'''
wgsl='''fn rockTexel(kind:i32,map:i32,level:i32,p:vec2<i32>)->vec4<f32>{
 let size=256>>u32(level);let xy=vec2<u32>(p & vec2<i32>(size-1));let offset=u32((65536-size*size)*4/3);
 return tableRead(1048580u+u32(kind)*262143u+u32(map)*87381u+offset+xy.y*u32(size)+xy.x);
}
fn rockMap(kind:i32,map:i32,uv:vec2<f32>,lod:f32)->vec4<f32>{
 let level=i32(clamp(floor(lod),0.,8.));let size=256>>u32(level);
 let p=fract(uv)*f32(size)-.5;let i=vec2<i32>(floor(p));let f=fract(p);
 return mix(mix(rockTexel(kind,map,level,i),rockTexel(kind,map,level,i+vec2<i32>(1,0)),f.x),mix(rockTexel(kind,map,level,i+vec2<i32>(0,1)),rockTexel(kind,map,level,i+vec2<i32>(1,1)),f.x),f.y);
}
fn rockDiffuse(rgb:vec3<f32>,metres:vec2<f32>,tag:i32,footprint:f32)->vec3<f32>{
 let kind=tag/6;let face=tag%6;let axis=face/2;
 var N=vec3<f32>(0);N[axis]=select(-1.,1.,face%2==0);
 var T=vec3<f32>(0);T[(axis+1)%3]=1.;var B=vec3<f32>(0);B[(axis+2)%3]=1.;
 let lod=clamp(log2(max(footprint*128.,1.)),0.,8.);
 let uv=metres/2.;let a=rockMap(kind,0,uv,lod);let packed=rockMap(kind,2,uv,lod);
 let nm=normalize(rockMap(kind,1,uv,lod).xyz*2.-1.);let n=normalize(T*nm.x+B*nm.y+N*nm.z);
 var L=cloudRow(9).xyz;L=L/max(length(L),.0001);
 let relief=mix(1.,(.30+.70*abs(dot(n,L)))/(.30+.70*abs(dot(N,L))),clamp(packed.r,0.,1.));
 let mean=select(vec3<f32>(.09831836,.15777506,.2661084),vec3<f32>(.2836261,.20529686,.12704249),kind==0);
 return rgb*(a.rgb/mean)*packed.a*clamp(relief,.55,1.55);
}
'''
for name in ['triangle.gl.frag','triangle.frag','scene.ios.frag','triangle.wgsl']:
 p=R/name;s=p.read_text()
 if START in s:
  a=s.index(START);b=s.index(END,a)+len(END);s=s[:a]+s[b:]
 if name.endswith('wgsl'):
  code=wgsl;anchor='@fragment fn fs_main';a=s.index(anchor)
 else:
  read=('vec4 rockRead(int n){return tableRead(1048580+n);}\n' if name!='scene.ios.frag' else 'vec4 rockRead(int n){int i=528*723+81920+147456+1048576+n;return texelFetch(cloudNoiseAtlas,ivec2(i%528,i/528),0);}\n')
  code=read+glsl;a=s.index('// BEGIN GENERATED MEADOW GRASS\n') if '// BEGIN GENERATED MEADOW GRASS\n' in s else s.index('vec4 surface(')
 s=s[:a]+START+code+END+s[a:];p.write_text(s)
