vec4 theatreTexel(int tile,ivec2 p){ivec2 xy=p & ivec2(127);int n=tile*16384+xy.y*128+xy.x;int row=workstationData.rows.length()-20480+n/4;return unpackUnorm4x8(floatBitsToUint(workstationData.rows[row][n%4]));}
vec4 theatreSample(vec4 d){
 vec2 p=fract(d.xy)*128.0-0.5; ivec2 i=ivec2(floor(p)); vec2 f=fract(p);
 int tile=d.w==114.?4:int(round(d.w))-100;
 vec4 c=mix(mix(theatreTexel(tile,i),theatreTexel(tile,i+ivec2(1,0)),f.x),
            mix(theatreTexel(tile,i+ivec2(0,1)),theatreTexel(tile,i+ivec2(1,1)),f.x),f.y);
 return vec4(mix(vec3(0.22,0.025,0.04),c.rgb,c.a)*d.z*vec3(1.08,1.0,0.9),1.0);
}

vec4 tableRead(int n){int b=workstationData.rows.length()-20480-1135961;return unpackUnorm4x8(floatBitsToUint(workstationData.rows[b+n/4][n%4]));}
vec3 tableEyeValue(){return workstationData.rows[workstationData.rows.length()-20480-1135961].xyz;}
vec4 tableTexel(int map,int level,ivec2 p){int size=512>>level;ivec2 xy=p & ivec2(size-1);int offset=(262144-size*size)*4/3;return tableRead(4+map*349525+offset+xy.y*size+xy.x);}
// UV walnut: linear-light diffuse + GGX, tangent-space normals and bounded relief.
// UV walnut: linear-light diffuse + GGX, tangent-space normals and bounded relief.
vec4 tableMap(int map,vec2 uv,float lod){
 int level=int(clamp(floor(lod),0.,9.));int size=512>>level;
 vec2 p=fract(uv)*float(size)-.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 return mix(mix(tableTexel(map,level,i),tableTexel(map,level,i+ivec2(1,0)),f.x),
            mix(tableTexel(map,level,i+ivec2(0,1)),tableTexel(map,level,i+ivec2(1,1)),f.x),f.y);
}
vec4 tableSurface(vec4 d,float footprint){
 int face=int(round(d.w))-106;
 vec3 ns[6]=vec3[6](vec3(0,1,0),vec3(0,-1,0),vec3(0,0,1),vec3(0,0,-1),vec3(1,0,0),vec3(-1,0,0));
 vec3 ts[6]=vec3[6](vec3(1,0,0),vec3(1,0,0),vec3(1,0,0),vec3(-1,0,0),vec3(0,0,-1),vec3(0,0,1));
 vec3 N=ns[face];vec3 T=ts[face];vec3 B=cross(N,T);
 vec3 p=T*d.x*2.+B*d.y*.65+N*d.z;
 vec3 V=normalize(tableEyeValue()-p);vec3 vt=vec3(dot(V,T),dot(V,B),dot(V,N));
 float lod=clamp(log2(max(footprint*512.,1.)),0.,9.);
 vec2 uv=d.xy;
 // Height perturbs sampling only; the modeled bevel owns the silhouette.
 vec2 travel=vt.xy/max(abs(vt.z),.2)*vec2(.0004/2.,.0004/.65);
 for(int step=0;step<4;step++){float h=tableMap(2,uv,lod).g-.5;uv=d.xy+travel*h;}
 vec4 packed=tableMap(2,uv,lod);float rough=clamp(packed.r,.24,.85);
 vec3 nm=normalize(tableMap(1,uv,lod).rgb*2.-1.);vec3 n=normalize(T*nm.x+B*nm.y+N*nm.z);
 vec3 albedo=pow(tableMap(0,uv,lod).rgb,vec3(2.2));
 vec3 color=albedo*.30*packed.a;
 vec3 lights[5]=vec3[5](vec3(-23,15,-24),vec3(23,15,-24),vec3(-18,15,18),vec3(18,15,18),vec3(0,13,-49));
 for(int j=0;j<5;j++){
  vec3 delta=lights[j]-p;vec3 L=normalize(delta);vec3 H=normalize(L+V);
  float nl=max(dot(n,L),0.);float nv=max(dot(n,V),.02);float nh=max(dot(n,H),0.);float vh=max(dot(V,H),0.);
  float a=rough*rough;float a2=a*a;float denom=nh*nh*(a2-1.)+1.;float D=a2/(3.14159265*denom*denom);
  float k=(rough+1.)*(rough+1.)/8.;float G=(nl/(nl*(1.-k)+k))*(nv/(nv*(1.-k)+k));
  vec3 F=vec3(.04)+vec3(.96)*pow(1.-vh,5.);
  vec3 spec=D*G*F/max(4.*nl*nv,.001);
  vec3 radiance=vec3(1.,.86,.68)*2.2/(1.+dot(delta,delta)/180.);
  color+=((vec3(1.)-F)*albedo/3.14159265+spec)*radiance*nl;
 }
 return vec4(pow(clamp(color,vec3(0),vec3(1)),vec3(1./2.2)),1.);
}
// BEGIN EXTERIOR LIMESTONE
vec4 exteriorFrame(int row){vec4 v;for(int j=0;j<4;j++){v[j]=uintBitsToFloat(packUnorm4x8(tableRead(2446684+row*4+j)));}return v;}

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
// END EXTERIOR LIMESTONE
// BEGIN RED CARPET
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
// END RED CARPET
