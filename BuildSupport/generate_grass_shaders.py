"""Shared bounded meadow material on GL/GLES/Vulkan/WebGPU. No new bindings."""
from pathlib import Path
R=Path(__file__).resolve().parents[1]/'Procedures/Shaders'
START='// BEGIN GENERATED MEADOW GRASS\n'; END='// END GENERATED MEADOW GRASS\n'
GLSL='''vec4 grassTexel(int map,int level,ivec2 p){
 int size=256>>level;ivec2 xy=p & ivec2(size-1);int offset=(65536-size*size)*4/3;
 return tableRead(1572868+map*87381+offset+xy.y*size+xy.x);
}
vec4 grassMap(int map,vec2 uv,float lod){
 int level=int(clamp(floor(lod),0.,8.));int size=256>>level;
 vec2 p=fract(uv)*float(size)-.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 return mix(mix(grassTexel(map,level,i),grassTexel(map,level,i+ivec2(1,0)),f.x),mix(grassTexel(map,level,i+ivec2(0,1)),grassTexel(map,level,i+ivec2(1,1)),f.x),f.y);
}
vec3 grassDiffuse(vec3 rgb,vec2 metres,float footprint){
 float lod=clamp(log2(max(footprint*512.,1.)),0.,8.);
 vec4 a=grassMap(0,metres*2.,lod);vec4 nh=grassMap(1,metres*2.,lod);
 // Match the dominant-plane metric UVs supplied by EnvironmentGeometry.
 // This approximates the ground frame; it does not displace terrain/collision.
 vec3 up=cloudRow(2).xyz;int axis=0;
 if(abs(up.y)>abs(up.x))axis=1;if(abs(up.z)>abs(up[axis]))axis=2;
 vec3 N=vec3(0);N[axis]=up[axis]<0.?-1.:1.;
 vec3 T=vec3(0);T[(axis+1)%3]=1.;vec3 B=vec3(0);B[(axis+2)%3]=1.;
 vec3 nm=normalize(nh.xyz*2.-1.);vec3 n=normalize(T*nm.x+B*nm.y+N*nm.z);
 vec3 L=cloudRow(9).xyz;L=L/max(length(L),.0001);
 float relief=clamp((.45+.55*max(dot(n,L),0.))/(.45+.55*max(dot(N,L),0.)),.65,1.35);
 // Color-normalized modulation preserves the engine's existing light and fog.
 // Fade to its mean before the material becomes subpixel; no distant sparkle.
 float detail=1.-smoothstep(4.,8.,lod);
 vec3 albedo=clamp(a.rgb/vec3(.167,.224,.067),vec3(.45),vec3(1.65));
 float cavity=mix(.90,1.,nh.a)*mix(.9,1.,a.a);
 return rgb*mix(vec3(1),albedo*relief*cavity,detail);
}
'''
WGSL='''fn grassTexel(map:i32,level:i32,p:vec2<i32>)->vec4<f32>{
 let size=256>>u32(level);let xy=vec2<u32>(p & vec2<i32>(size-1));let offset=u32((65536-size*size)*4/3);
 return tableRead(1572868u+u32(map)*87381u+offset+xy.y*u32(size)+xy.x);
}
fn grassMap(map:i32,uv:vec2<f32>,lod:f32)->vec4<f32>{
 let level=i32(clamp(floor(lod),0.,8.));let size=256>>u32(level);
 let p=fract(uv)*f32(size)-.5;let i=vec2<i32>(floor(p));let f=fract(p);
 return mix(mix(grassTexel(map,level,i),grassTexel(map,level,i+vec2<i32>(1,0)),f.x),mix(grassTexel(map,level,i+vec2<i32>(0,1)),grassTexel(map,level,i+vec2<i32>(1,1)),f.x),f.y);
}
fn grassDiffuse(rgb:vec3<f32>,metres:vec2<f32>,footprint:f32)->vec3<f32>{
 let lod=clamp(log2(max(footprint*512.,1.)),0.,8.);
 let a=grassMap(0,metres*2.,lod);let nh=grassMap(1,metres*2.,lod);
 let up=cloudRow(2).xyz;var axis=0;
 if(abs(up.y)>abs(up.x)){axis=1;}if(abs(up.z)>abs(up[axis])){axis=2;}
 var N=vec3<f32>(0);N[axis]=select(1.,-1.,up[axis]<0.);
 var T=vec3<f32>(0);T[(axis+1)%3]=1.;var B=vec3<f32>(0);B[(axis+2)%3]=1.;
 let nm=normalize(nh.xyz*2.-1.);let n=normalize(T*nm.x+B*nm.y+N*nm.z);
 var L=cloudRow(9).xyz;L=L/max(length(L),.0001);
 let relief=clamp((.45+.55*max(dot(n,L),0.))/(.45+.55*max(dot(N,L),0.)),.65,1.35);
 let detail=1.-smoothstep(4.,8.,lod);
 let albedo=clamp(a.rgb/vec3<f32>(.167,.224,.067),vec3<f32>(.45),vec3<f32>(1.65));
 let cavity=mix(.90,1.,nh.a)*mix(.9,1.,a.a);
 return rgb*mix(vec3<f32>(1),albedo*relief*cavity,detail);
}
'''
for name in ['triangle.gl.frag','triangle.frag','scene.ios.frag','triangle.wgsl']:
    p=R/name; s=p.read_text(); wg=name.endswith('wgsl')
    if START in s:
        a=s.index(START);b=s.index(END,a)+len(END);s=s[:a]+s[b:]
    a=s.index('@fragment fn fs_main' if wg else 'vec4 surface(')
    code=WGSL if wg else GLSL
    if name=='scene.ios.frag':code=code.replace('tableRead(1572868+', 'rockRead(524288+')
    s=s[:a]+START+code+END+s[a:]
    # Keep soilNoise/dirt behavior, replacing only the selected green material.
    if wg:
        s=s.replace('uv:vec2<f32>,fade:f32)->vec3<f32> {','uv:vec2<f32>,fade:f32,footprint:f32)->vec3<f32> {')
        needle=' let grass=smoothstep(0.02,0.10,rgb.g-rgb.r);'
        if 'if grass>0.5' not in s:
            s=s.replace(needle,needle+'\n if grass>0.5{return grassDiffuse(rgb,uv,footprint);}')
        s=s.replace('soilDiffuse(i.data.rgb,i.soilUV,soilFade)','soilDiffuse(i.data.rgb,i.soilUV,soilFade,rockFootprint)')
    else:
        # Forward declarations because the existing dirt helper precedes storage.
        declaration='vec3 grassDiffuse(vec3 rgb,vec2 metres,float footprint);\n'
        if declaration not in s:s=s.replace('vec3 soilDiffuse(',declaration+'vec3 soilDiffuse(',1)
        needle=' float grass=smoothstep(0.02,0.10,rgb.g-rgb.r);'
        if 'if(grass>0.5)' not in s:
            s=s.replace(needle,needle+'\n if(grass>0.5)return grassDiffuse(rgb,uv,max(length(dFdx(uv)),length(dFdy(uv))));')
    if not wg:
        s=s.replace('soilDiffuse(vec3 rgb,vec2 uv)', 'soilDiffuse(vec3 rgb,vec2 uv,float footprint)')
        s=s.replace('grassDiffuse(rgb,uv,max(length(dFdx(uv)),length(dFdy(uv))))', 'grassDiffuse(rgb,uv,footprint)')
        s=s.replace('smoothstep(0.008,0.035,max(length(dFdx(uv)),length(dFdy(uv))))', 'smoothstep(0.008,0.035,footprint)')
        s=s.replace('soilDiffuse(data.rgb,soilUV)', 'soilDiffuse(data.rgb,soilUV,rockFootprint)')
    p.write_text(s)
