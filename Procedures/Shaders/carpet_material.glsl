// Metric red wool; immutable offline maps, shallow relief, five theatre lamps.
vec4 carpetLevel(int map,vec2 uv,int level){
 int size=512>>level;vec2 p=fract(uv)*float(size)-.5;ivec2 ip=ivec2(floor(p));vec2 f=fract(p);
 int offset=3495268+map*349525+(262144-size*size)*4/3;
 ivec2 a=ip&ivec2(size-1);ivec2 b=(ip+ivec2(1,0))&ivec2(size-1);
 ivec2 c=(ip+ivec2(0,1))&ivec2(size-1);ivec2 d=(ip+ivec2(1,1))&ivec2(size-1);
 return mix(mix(tableRead(offset+a.y*size+a.x),tableRead(offset+b.y*size+b.x),f.x),mix(tableRead(offset+c.y*size+c.x),tableRead(offset+d.y*size+d.x),f.x),f.y);
}
vec4 carpetMap(int map,vec2 uv,float lod){
 int lo=int(floor(lod));return mix(carpetLevel(map,uv,lo),carpetLevel(map,uv,min(lo+1,9)),fract(lod));
}
vec4 carpetSurface(vec4 data,float footprint){
 vec3 p=data.xyz;float slope=data.w==118.?5.5/66.:data.w==119.?5./12.:data.w==120.?-6./17.:0.;
 vec3 N=normalize(vec3(0,1,slope));vec3 T=vec3(1,0,0);vec3 B=cross(N,T);
 vec3 V=normalize(tableEyeValue()-p);vec3 vt=vec3(dot(V,T),dot(V,B),dot(V,N));
 vec2 baseUV=vec2(dot(p,T),dot(p,B))/.256;
 float lod=clamp(log2(max(footprint*512./.256,1.)),0.,9.);
 vec2 uv=baseUV;vec2 travel=vt.xy/max(abs(vt.z),.2)*(.0012/.256);
 for(int j=0;j<4;j++){uv=baseUV+travel*(carpetMap(2,uv,lod).g-.5);}
 vec4 packed=carpetMap(2,uv,lod);float rough=clamp(packed.r,.86,.99);
 vec3 nm=normalize(carpetMap(1,uv,lod).rgb*2.-1.);vec3 n=normalize(T*nm.x+B*nm.y+N*nm.z);
 vec3 albedo=pow(carpetMap(0,uv,lod).rgb,vec3(2.2));vec3 color=albedo*.30*packed.a;
 vec3 lamps[5]=vec3[5](vec3(-23,15,-24),vec3(23,15,-24),vec3(-18,15,18),vec3(18,15,18),vec3(0,13,-49));
 for(int j=0;j<5;j++){
  vec3 delta=lamps[j]-p;vec3 L=normalize(delta);vec3 H=normalize(L+V);
  float nl=max(dot(n,L),0.);float nv=max(dot(n,V),.02);float nh=max(dot(n,H),0.);float vh=max(dot(V,H),0.);
  float a=rough*rough;float a2=a*a;float denom=nh*nh*(a2-1.)+1.;float D=a2/(3.14159265*denom*denom);
  float k=(rough+1.)*(rough+1.)/8.;float G=nl/(nl*(1.-k)+k)*nv/(nv*(1.-k)+k);
  vec3 F=vec3(.025)+vec3(.975)*pow(1.-vh,5.);vec3 spec=D*G*F/max(4.*nl*nv,.001);
  // A restrained red-tinted grazing sheen approximates the Blender wool lobe.
  vec3 sheen=albedo*.08*pow(1.-vh,5.);
  vec3 radiance=vec3(1,.86,.68)*2.2/(1.+dot(delta,delta)/180.);
  color+=((vec3(1.)-F)*albedo/3.14159265+spec+sheen)*radiance*nl;
 }
 return vec4(pow(clamp(color,vec3(0),vec3(1)),vec3(1./2.2)),1.);
}
