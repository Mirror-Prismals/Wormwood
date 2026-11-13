"""Install the shared native limestone material without changing existing material equations."""
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]/'Procedures/Shaders'
GLSL='''
vec4 exteriorMap(int map,vec2 uv,float lod){
 int level=int(clamp(floor(lod),0.,9.));int size=512>>level;
 vec2 p=fract(uv)*float(size)-.5;ivec2 ip=ivec2(floor(p));vec2 f=fract(p);
 int offset=2446692+map*349525+(262144-size*size)*4/3;
 ivec2 a=ip & ivec2(size-1);ivec2 b=(ip+ivec2(1,0)) & ivec2(size-1);
 ivec2 c=(ip+ivec2(0,1)) & ivec2(size-1);ivec2 d=(ip+ivec2(1,1)) & ivec2(size-1);
 return mix(mix(tableRead(offset+a.y*size+a.x),tableRead(offset+b.y*size+b.x),f.x),mix(tableRead(offset+c.y*size+c.x),tableRead(offset+d.y*size+d.x),f.x),f.y);
}
vec4 exteriorSurface(vec4 data,float footprint){
 vec3 p=data.xyz;vec3 radial=normalize(vec3(p.x,0,p.z));vec3 N=data.w==113.?-radial:radial;if(data.w>=115.)N=vec3(0,0,data.w==115.?-1.:1.);vec3 T=data.w>=115.?vec3(1,0,0):vec3(radial.z,0,-radial.x);vec3 B=vec3(0,1,0);
 vec3 V=normalize(tableEyeValue()-p);vec3 vt=vec3(dot(V,T),dot(V,B),dot(V,N));
 vec2 metric=vec2(4.775220833456486,2.4);
 vec2 baseUV=data.w>=115.?p.xy/metric:vec2(atan(-p.z,p.x)*100./6.28318530718,p.y/2.4);
 float lod=clamp(log2(max(footprint*512./2.4,1.)),0.,9.);
 vec2 uv=baseUV;vec2 travel=vt.xy/max(abs(vt.z),.2)*.006/metric;
 for(int j=0;j<4;j++){uv=baseUV+travel*(exteriorMap(2,uv,lod).g-.5);}
 vec4 packed=exteriorMap(2,uv,lod);float rough=clamp(packed.r,.68,.96);
 vec3 nm=normalize(exteriorMap(1,uv,lod).rgb*2.-1.);vec3 n=normalize(T*nm.x+B*nm.y+N*nm.z);
 vec3 albedo=pow(exteriorMap(0,uv,lod).rgb,vec3(2.2));
 vec4 direct=exteriorFrame(0);vec3 tint=exteriorFrame(1).rgb;
 vec3 L=normalize(direct.xyz);vec3 H=normalize(L+V);
 float nl=max(dot(n,L),0.);float nv=max(dot(n,V),.02);float nh=max(dot(n,H),0.);float vh=max(dot(V,H),0.);
 float a=rough*rough;float a2=a*a;float denom=nh*nh*(a2-1.)+1.;float D=a2/(3.14159265*denom*denom);
 float k=(rough+1.)*(rough+1.)/8.;float G=nl/(nl*(1.-k)+k)*nv/(nv*(1.-k)+k);
 vec3 F=vec3(.035)+vec3(.965)*pow(1.-vh,5.);vec3 spec=D*G*F/max(4.*nl*nv,.001);
 vec3 color=(albedo*(.25*packed.a+nl)+spec*nl)*tint*direct.w;
 if(data.w==113. || data.w>=115.){vec3 shade=vec3(.34,.30,.26);vec3 lamps[5]=vec3[5](vec3(-23,15,-24),vec3(23,15,-24),vec3(-18,15,18),vec3(18,15,18),vec3(0,13,-49));for(int j=0;j<5;j++){vec3 d=lamps[j]-p;shade+=max(dot(n,normalize(d)),0.)*1.2/(1.+dot(d,d)/250.)*vec3(1,.86,.68);}color=albedo*shade*packed.a;}
 return vec4(pow(clamp(color,vec3(0),vec3(1)),vec3(1./2.2)),1.);
}
'''
WGSL='''
fn exteriorMap(map:i32,uv:vec2<f32>,lod:f32)->vec4<f32>{
 let level=i32(clamp(floor(lod),0.,9.));let size=512>>u32(level);
 let p=fract(uv)*f32(size)-.5;let ip=vec2<i32>(floor(p));let f=fract(p);
 let offset=2446692+map*349525+(262144-size*size)*4/3;
 let a=ip & vec2<i32>(size-1);let b=(ip+vec2<i32>(1,0)) & vec2<i32>(size-1);
 let c=(ip+vec2<i32>(0,1)) & vec2<i32>(size-1);let d=(ip+vec2<i32>(1,1)) & vec2<i32>(size-1);
 return mix(mix(tableRead(u32(offset+a.y*size+a.x)),tableRead(u32(offset+b.y*size+b.x)),f.x),mix(tableRead(u32(offset+c.y*size+c.x)),tableRead(u32(offset+d.y*size+d.x)),f.x),f.y);
}
fn exteriorSurface(data:vec4<f32>,footprint:f32)->vec4<f32>{
 let p=data.xyz;let radial=normalize(vec3<f32>(p.x,0,p.z));var N=select(radial,-radial,data.w==113.);if data.w>=115.{N=vec3<f32>(0,0,select(1.,-1.,data.w==115.));}let T=select(vec3<f32>(radial.z,0,-radial.x),vec3<f32>(1,0,0),data.w>=115.);let B=vec3<f32>(0,1,0);
 let V=normalize(tableEyeValue()-p);let vt=vec3<f32>(dot(V,T),dot(V,B),dot(V,N));
 let metric=vec2<f32>(4.775220833456486,2.4);
 let baseUV=select(vec2<f32>(atan2(-p.z,p.x)*100./6.28318530718,p.y/2.4),p.xy/metric,data.w>=115.);
 let lod=clamp(log2(max(footprint*512./2.4,1.)),0.,9.);
 var uv=baseUV;let travel=vt.xy/max(abs(vt.z),.2)*.006/metric;
 for(var j=0;j<4;j++){uv=baseUV+travel*(exteriorMap(2,uv,lod).g-.5);}
 let packed=exteriorMap(2,uv,lod);let rough=clamp(packed.r,.68,.96);
 let nm=normalize(exteriorMap(1,uv,lod).rgb*2.-1.);let n=normalize(T*nm.x+B*nm.y+N*nm.z);
 let albedo=pow(exteriorMap(0,uv,lod).rgb,vec3<f32>(2.2));
 let direct=exteriorFrame(0);let tint=exteriorFrame(1).rgb;
 let L=normalize(direct.xyz);let H=normalize(L+V);
 let nl=max(dot(n,L),0.);let nv=max(dot(n,V),.02);let nh=max(dot(n,H),0.);let vh=max(dot(V,H),0.);
 let a=rough*rough;let a2=a*a;let denom=nh*nh*(a2-1.)+1.;let D=a2/(3.14159265*denom*denom);
 let k=(rough+1.)*(rough+1.)/8.;let G=nl/(nl*(1.-k)+k)*nv/(nv*(1.-k)+k);
 let F=vec3<f32>(.035)+vec3<f32>(.965)*pow(1.-vh,5.);let spec=D*G*F/max(4.*nl*nv,.001);
 var color=(albedo*(.25*packed.a+nl)+spec*nl)*tint*direct.w;
 if data.w==113. || data.w>=115.{var shade=vec3<f32>(.34,.30,.26);let lamps=array<vec3<f32>,5>(vec3<f32>(-23,15,-24),vec3<f32>(23,15,-24),vec3<f32>(-18,15,18),vec3<f32>(18,15,18),vec3<f32>(0,13,-49));for(var j=0;j<5;j++){let d=lamps[j]-p;shade+=max(dot(n,normalize(d)),0.)*1.2/(1.+dot(d,d)/250.)*vec3<f32>(1,.86,.68);}color=albedo*shade*packed.a;}
 return vec4<f32>(pow(clamp(color,vec3<f32>(0),vec3<f32>(1)),vec3<f32>(1./2.2)),1.);
}
'''
START='// BEGIN EXTERIOR LIMESTONE\n';END='// END EXTERIOR LIMESTONE\n'
for name in ['triangle.wgsl','triangle.frag','triangle.gl.frag','theatre_camera.wgsl','theatre_camera.glsl','theatre_camera.vulkan.glsl']:
 p=ROOT/name;s=p.read_text();wg=name.endswith('wgsl');camera=name.startswith('theatre_camera')
 if START in s:s=s[:s.index(START)]+s[s.index(END)+len(END):]
 if wg:
  frame='fn exteriorFrame(row:i32)->vec4<f32>{var v:vec4<f32>;for(var j=0;j<4;j++){v[j]=bitcast<f32>(pack4x8unorm(tableRead(u32(2446684+row*4+j))));}return v;}\n'
 else:
  frame='vec4 exteriorFrame(int row){vec4 v;for(int j=0;j<4;j++){v[j]=uintBitsToFloat(packUnorm4x8(tableRead(2446684+row*4+j)));}return v;}\n'
  if name.endswith('.gl.frag') or name=='theatre_camera.glsl':frame='uniform vec4 exteriorLight[2];\nvec4 exteriorFrame(int row){return exteriorLight[row];}\n'
 block=START+frame+(WGSL if wg else GLSL)+END
 if camera:s=s.rstrip()+'\n'+block
 else:
  marker='@fragment fn fs_main' if wg else 'vec4 surface('
  pos=s.index(marker);s=s[:pos]+block+s[pos:]
  if wg:
   needle=' if i.material>=106.0'
   if 'return exteriorSurface(i.data,' not in s:s=s.replace(needle,' if (i.material>=112. && i.material<=116. && i.material!=114.){return exteriorSurface(i.data,max(length(dpdx(i.data.xyz)),length(dpdy(i.data.xyz))));}\n'+needle)
  else:
   needle=' if(material>=106.0'
   if 'return exteriorSurface(data,' not in s:s=s.replace(needle,' if(material>=112. && material<=116. && material!=114.)return exteriorSurface(data,max(length(dFdx(data.xyz)),length(dFdy(data.xyz))));\n'+needle)
 p.write_text(s)
# Embedded camera shares the same material, with derivatives outside selection.
p=ROOT/'generate_workstation.py';s=p.read_text()
if 'return exteriorSurface(input.cameraData,' not in s:s=s.replace('if(input.cameraData.w>=106.0', 'if(input.cameraData.w>=112. && input.cameraData.w<=116. && input.cameraData.w!=114.){return exteriorSurface(input.cameraData,max(length(dpdx(input.cameraData.xyz)),length(dpdy(input.cameraData.xyz))));}if(input.cameraData.w>=106.0')
if 'pixel=exteriorSurface(cameraData,' not in s:s=s.replace('if(cameraData.w>=106.0','if(cameraData.w>=112. && cameraData.w<=116. && cameraData.w!=114.){pixel=exteriorSurface(cameraData,max(length(dFdx(cameraData.xyz)),length(dFdy(cameraData.xyz))));return;}if(cameraData.w>=106.0')
p.write_text(s)
p=ROOT.parents[1]/'Render/WorkstationCameraColor.wgsl';s=p.read_text()
if 'return exteriorSurface(input.cameraData,' not in s:s=s.replace('if(input.cameraData.w>=106.0','if(input.cameraData.w>=112. && input.cameraData.w<=116. && input.cameraData.w!=114.){return exteriorSurface(input.cameraData,max(length(dpdx(input.cameraData.xyz)),length(dpdy(input.cameraData.xyz))));}if(input.cameraData.w>=106.0');p.write_text(s)
import runpy
runpy.run_path(str(ROOT/'generate_workstation.py'),run_name='__main__')
