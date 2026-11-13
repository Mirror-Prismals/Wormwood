#version 330 core
in vec4 data;in vec2 screen;flat in float material;out vec4 pixel;uniform sampler2D playerTexture;
vec4 playerSample(vec2 uv){return texture(playerTexture,uv);}
vec4 colorEmotion(vec4 effect, float kind, vec2 screen, float aspect) {
 vec2 p=vec2(screen.x*aspect,screen.y*1.55);
 float r=length(p);float edge=pow(smoothstep(1.48,2.10,r),0.85);
 float pulse=0.92+0.08*sin(effect.y*2.2+r*8.0);
 float charge=clamp(effect.z,0.0,1.0);
 vec3 a=kind < -4.5?vec3(1.0,0.55,0.10):vec3(0.10,0.85,0.20);
 vec3 b=kind < -4.5?vec3(0.10,0.85,0.20):vec3(1.0,0.0,1.0);
 vec3 color=mix(a,b,charge);
 if(charge>=0.999){float angle=atan(p.y,p.x);color=mix(b,a,smoothstep(-0.01,0.01,cos(angle-effect.y*3.0)));}

 float alpha=clamp(edge*pulse*effect.x,0.0,0.98)*0.10;
 return vec4(color,alpha);
}

in vec2 soilUV;
float soilNoise(vec2 p, float period) {
 vec2 i=floor(p), t=fract(p); t=t*t*(3.0-2.0*t);
 vec4 h=mod(vec4(i.x,i.x+1.0,i.x,i.x+1.0),period)*73.0+
        mod(vec4(i.y,i.y,i.y+1.0,i.y+1.0),period)*151.0;
 h=fract(h*0.1031); h=fract(h*(h+33.33)*(h+h));
 return mix(mix(h.x,h.y,t.x),mix(h.z,h.w,t.x),t.y)-0.5;
}
vec3 grassDiffuse(vec3 rgb,vec2 metres,float footprint);
vec3 dirtDiffuse(vec3 rgb,vec2 metres,float footprint);
vec3 soilDiffuse(vec3 rgb,vec2 uv,float footprint) {
 float grass=smoothstep(0.02,0.10,rgb.g-rgb.r);
 if(grass>0.5)return mix(rgb,grassDiffuse(rgb,uv,footprint),1.0-smoothstep(192.0,320.0,1.0/gl_FragCoord.w));
 if(rgb.r>rgb.g*1.28 && rgb.b<rgb.g*.85)return mix(rgb,dirtDiffuse(rgb,uv,footprint),1.0-smoothstep(192.0,320.0,1.0/gl_FragCoord.w));
 float mottling=soilNoise(uv*4.0,16.0);
 float grains=soilNoise(uv*32.0,128.0);
 float blades=soilNoise(vec2(uv.x*48.0+uv.y*24.0,uv.y*24.0),96.0);
 float fade=1.0-smoothstep(0.008,0.035,footprint);
 return mix(rgb, rgb*(1.0+0.42*mottling+fade*(0.34*grains+0.48*grass*blades))
       +grass*vec3(0.043,0.034,0.008)*max(0.0,mottling+blades)*fade, 1.0-smoothstep(192.0,320.0,1.0/gl_FragCoord.w));
}

uniform highp sampler2D cloudNoiseAtlas;
vec4 theatreTexel(int tile,ivec2 p){ivec2 xy=p & ivec2(127);int n=tile*16384+xy.y*128+xy.x;int index=528*723+n;return texelFetch(cloudNoiseAtlas,ivec2(index%528,index/528),0);}
vec4 theatreSample(vec4 d){
 vec2 p=fract(d.xy)*128.0-0.5; ivec2 i=ivec2(floor(p)); vec2 f=fract(p);
 int tile=d.w==114.?4:int(round(d.w))-100;
 vec4 c=mix(mix(theatreTexel(tile,i),theatreTexel(tile,i+ivec2(1,0)),f.x),
            mix(theatreTexel(tile,i+ivec2(0,1)),theatreTexel(tile,i+ivec2(1,1)),f.x),f.y);
 return vec4(mix(vec3(0.22,0.025,0.04),c.rgb,c.a)*d.z*vec3(1.08,1.0,0.9),1.0);
}
uniform vec4 tableEye;
vec3 tableEyeValue(){return tableEye.xyz;}
vec4 tableRead(int n){int i=528*723+81920+147456+n-4;return texelFetch(cloudNoiseAtlas,ivec2(i%528,i/528),0);}
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

// BEGIN GENERATED CAVE ROCK
vec4 rockRead(int n){return tableRead(1048580+n);}
vec4 cloudRow(int index);
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
// END GENERATED CAVE ROCK
// BEGIN GENERATED MEADOW GRASS
vec4 grassTexel(int map,int level,ivec2 p){
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
// END GENERATED MEADOW GRASS
// BEGIN GENERATED FINE LOAM
vec4 dirtTexel(int map,int level,ivec2 p){
 int size=512>>level;ivec2 xy=p & ivec2(size-1);int offset=(262144-size*size)*4/3;
 return tableRead(1747632+map*349525+offset+xy.y*size+xy.x);
}
vec4 dirtMap(int map,vec2 uv,float lod){
 int level=int(clamp(floor(lod),0.,9.));int size=512>>level;
 vec2 p=fract(uv)*float(size)-.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 return mix(mix(dirtTexel(map,level,i),dirtTexel(map,level,i+ivec2(1,0)),f.x),mix(dirtTexel(map,level,i+ivec2(0,1)),dirtTexel(map,level,i+ivec2(1,1)),f.x),f.y);
}
vec3 dirtDiffuse(vec3 rgb,vec2 metres,float footprint){
 float lod=clamp(log2(max(footprint*1024.,1.)),0.,9.);
 vec4 a=dirtMap(0,metres*2.,lod);vec4 nh=dirtMap(1,metres*2.,lod);
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
 float detail=1.-smoothstep(5.,9.,lod);
 vec3 albedo=clamp(a.rgb/vec3(0.29019608,0.20784314,0.12156863),vec3(.45),vec3(1.65));
 float cavity=mix(.90,1.,nh.a)*mix(.9,1.,a.a);
 return rgb*mix(vec3(1),albedo*relief*cavity,detail);
}
// END GENERATED FINE LOAM
// BEGIN EXTERIOR LIMESTONE
uniform vec4 exteriorLight[2];
vec4 exteriorFrame(int row){return exteriorLight[row];}

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
vec4 surface(vec4 data, float material) {
 float rockFootprint=max(length(dFdx(soilUV)),length(dFdy(soilUV)));
 if(material<=-4096.)return vec4(rockDiffuse(data.rgb,soilUV,int(-material-4096.),rockFootprint),1.);
 float tableFootprint=max(length(dFdx(data.xy)),length(dFdy(data.xy)));
 if(((material>=112. && material<=116. && material!=114.)))return exteriorSurface(data,max(length(dFdx(data.xyz)),length(dFdy(data.xyz))));
 if(material>=117. && material<=120.)return carpetSurface(data,max(length(dFdx(data.xyz)),length(dFdy(data.xyz))));
 if(material>=106.0 && material<=111.0)return tableSurface(data,tableFootprint);
 if(material==105.0){ivec2 xy=ivec2(clamp(data.xy*vec2(512.,288.),vec2(0.),vec2(511.,287.))); int n=528*723+81920+xy.y*512+xy.x; return vec4(texelFetch(cloudNoiseAtlas,ivec2(n%528,n/528),0).rgb,1.);}
 if(material>=100.0 && (material<=104.0 || material==114.0))return theatreSample(data);
 if(material>=4096.0) return vec4(soilDiffuse(data.rgb,soilUV,rockFootprint),1.0);
 if(material < -1.5 && material > -2.5) { vec4 texel=playerSample(data.xy); if(texel.a<=0.002) discard; return vec4(texel.rgb*data.z,texel.a); }
 if(material < -0.5) {
  vec2 cell=data.xy*24.0;
  vec2 width=max(fwidth(cell),vec2(0.00001));
  // Cardinal: fract(UV * 24) < 0.03, filtered analytically at pixel scale.
  vec2 line=clamp((vec2(0.03)-fract(cell))/width+vec2(0.5),0.0,1.0);
  float coverage=max(line.x,line.y)*(1.0-smoothstep(0.35,1.0,max(width.x,width.y)));
  vec3 base=material < -2.5 ? vec3(0.45,0.29,0.13) : vec3(0.27,0.40,0.22);
  return vec4(base*data.z*(1.0-coverage),1.0);
 }
 return vec4(data.rgb,1.0);
}
in float viewDistance;
// BEGIN GENERATED CLOUDS
uniform vec4 cloudFrame[12];
vec4 cloudNoiseFetch(ivec2 p){return texelFetch(cloudNoiseAtlas,clamp(p,ivec2(0),ivec2(527,722)),0);}
vec4 cloudRow(int i){return cloudFrame[i];}
// Canonical cloud equations, translated into each native scene fragment.
// Noise/mip sampling, layer shapes and scattering follow the original engine.
// Spherical intervals and authored initial coverage are Wormwood adaptations.
float cloudPick(float a,float b,bool condition){if(condition)return b;return a;}
float cloudOffset(int level){if(level==0)return 0.0;if(level==1)return 528.0;if(level==2)return 664.0;if(level==3)return 700.0;if(level==4)return 710.0;if(level==5)return 716.0;return 720.0;}
vec4 cloudBilinear(vec2 point){
 vec2 p=point-0.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 return mix(mix(cloudNoiseFetch(i),cloudNoiseFetch(i+ivec2(1,0)),f.x),mix(cloudNoiseFetch(i+ivec2(0,1)),cloudNoiseFetch(i+ivec2(1,1)),f.x),f.y);
}
vec4 cloudSlice(vec2 uv,float slice,int level){
 float side=float(64>>level);float tileSize=side+2.0;float columns=min(8.0,side);
 float index=slice-floor(slice/side)*side;
 vec2 tile=vec2(index-floor(index/columns)*columns,floor(index/columns));
 return cloudBilinear(tile*tileSize+vec2(1.0,cloudOffset(level)+1.0)+fract(uv)*side);
}
vec4 cloudVolumeLevel(vec3 p,int level){
 vec3 q=fract(p);float side=float(64>>level);float z=q.z*side-0.5;float lo=floor(z);
 return mix(cloudSlice(q.xy,lo,level),cloudSlice(q.xy,lo+1.0,level),fract(z));
}
vec4 cloudVolume(vec3 p,float lod){
 float mip=clamp(lod,0.0,6.0);int lo=int(floor(mip));float f=fract(mip);
 vec4 low=cloudVolumeLevel(p,lo);if(f<0.02||lo==6)return low;
 return mix(low,cloudVolumeLevel(p,min(lo+1,6)),f);
}
float cloudLayerDensity(vec3 world,int layer,bool detailed,float footprint){
 vec4 profile=cloudRow(6+layer);float coverage=profile.z;float convection=profile.w;
 if(coverage<0.008)return 0.0;
 float altitude=length(world)-6371000.0;
 float height=(altitude-profile.x)/max(profile.y,1.0);
 if(height<=0.0||height>=1.0)return 0.0;
 vec4 wind=cloudRow(4);vec4 upper=cloudRow(5);float evolution=wind.z/512.0;
 float continuousLod=log2(max(1.0,footprint/64.0));float lod=cloudPick(round(continuousLod),continuousLod,detailed);
 vec3 p=vec3(world.x,altitude,world.z);
 vec3 q=vec3(p.x-wind.x,p.y-profile.x,p.z-wind.y)/4096.0;
 float shape=0.0;float vertical=0.0;float optical=1.0;
 if(layer==0){
  q.y=q.y*mix(0.30,0.85,convection);
  float anvil=smoothstep(0.68,0.95,height)*smoothstep(0.65,1.0,convection);
  q.x=q.x*(1.0-anvil*0.30);q.z=q.z*(1.0-anvil*0.30);
  float column=cloudVolume(vec3(q.x*0.5,0.37,q.z*0.5),max(0.0,lod-1.0)).a;
  float columnMask=smoothstep(mix(0.72,0.24,coverage),mix(0.84,0.40,coverage),column);
  if(columnMask<0.001&&coverage<0.78)return 0.0;
  vec4 warp=vec4(0.5);
  if(detailed)warp=cloudVolume(q*0.5+vec3(0.0,evolution*0.25,evolution*0.5),max(0.0,lod-1.0));
  vec4 n=cloudVolume(q+(warp.gba-0.5)*0.14,lod);
  float towerTop=mix(0.40,1.0,smoothstep(0.30,0.70,column));
  float threshold=mix(0.76,0.18,coverage)+convection*pow(height/towerTop,2.0)*(0.20-anvil*0.15);
  shape=smoothstep(threshold-0.075,threshold+0.13,n.r)*columnMask;
  float sheet=smoothstep(0.78,0.98,coverage)*(1.0-convection);
  shape=mix(shape,mix(0.70,1.0,n.a),sheet*0.65);
  vertical=smoothstep(0.0,mix(0.08,0.16,convection),height)*(1.0-smoothstep(mix(0.60,0.80,convection),1.0,height));
  if(detailed&&shape>0.001){
   float erosion=(1.0-n.g)*0.65+(1.0-n.b)*0.35;
   float edge=1.0-smoothstep(0.05,0.80,shape);float detailFade=1.0-smoothstep(80.0,350.0,footprint);
   shape=clamp(shape-erosion*edge*mix(0.18,0.62,convection)*wind.w*detailFade,0.0,1.0);
  }
  optical=mix(0.58,1.0,convection);
 }else if(layer==1){
  q=vec3(p.x-upper.x,(p.y-profile.x)*0.35,p.z-upper.y)/4096.0;
  vec4 warp=vec4(0.5);if(detailed)warp=cloudVolume(q*0.5+vec3(evolution*0.25,0.0,evolution*0.5),max(0.0,lod-1.0));
  vec4 n=cloudVolume(q*2.0+(warp.gba-0.5)*0.20,lod+1.0);
  float threshold=mix(0.78,0.20,coverage);shape=smoothstep(threshold-0.07,threshold+0.16,n.g*0.62+n.a*0.38);
  float sheet=smoothstep(0.65,0.97,coverage);shape=mix(shape,mix(0.55,0.95,n.r),sheet*0.80);
  vertical=smoothstep(0.0,0.13,height)*(1.0-smoothstep(0.55,1.0,height));optical=0.38;
 }else{
  float angle=0.32175055;
  float along=p.x*cos(angle)+p.z*sin(angle)-upper.z;
  float across=-p.x*sin(angle)+p.z*cos(angle)-upper.w;
  q=vec3(along*0.25+(height-0.5)*1000.0,(p.y-profile.x)*2.0,across*2.0)/4096.0;
  vec4 warp=vec4(0.5);if(detailed)warp=cloudVolume(q*0.5+vec3(evolution*0.25,evolution*0.5,0.0),lod);
  vec4 n=cloudVolume(q+(warp.gba-0.5)*0.28,lod+1.0);
  float filament=0.5+0.5*sin(q.z*50.265482+warp.a*7.0+n.a*3.0)*(1.0-smoothstep(60.0,260.0,footprint));
  float wisps=n.g*0.52+n.a*0.30+filament*0.18;
  shape=smoothstep(mix(0.78,0.24,coverage),mix(0.92,0.72,coverage),wisps);
  shape=mix(shape,mix(0.30,0.80,n.a),smoothstep(0.78,1.0,coverage)*0.65);
  vertical=smoothstep(0.0,0.18,height)*(1.0-smoothstep(0.50,1.0,height));optical=0.12;
 }
 return max(0.0,shape*vertical*optical*cloudRow(11).w);
}
float cloudDensity(vec3 p,bool detailed,float footprint){
 float density=0.0;for(int layer=0;layer<3;layer++){density+=cloudLayerDensity(p,layer,detailed,footprint);}return density;
}
float cloudPhase(float cosine,float g){return (1.0-g*g)/pow(max(1.0+g*g-2.0*g*cosine,0.03),1.5);}
vec3 cloudLight(vec3 p,vec3 rd,vec3 sun,float density){
 float opticalDepth=0.0;float distance=75.0;float stride=150.0;
 for(int probe=0;probe<2;probe++){opticalDepth+=cloudDensity(p+sun*distance,false,200.0)*stride;distance+=stride;stride*=2.4;}
 float directTransmission=exp(-opticalDepth*0.75);float multipleScatter=exp(-opticalDepth*0.18)*0.22;
 float cosine=clamp(dot(rd,sun),-1.0,1.0);
 float phase=min(cloudPhase(cosine,0.64)*0.75+cloudPhase(cosine,-0.22)*0.25,7.0);
 float powder=1.0-exp(-density*220.0);float silver=mix(1.0,1.0+powder*0.60,smoothstep(0.2,0.95,cosine));
 vec3 ambientTint=mix(vec3(0.48,0.58,0.72),max(cloudRow(11).rgb,vec3(0.02)),0.55);
 vec3 ambient=ambientTint*cloudRow(10).w*(0.95+multipleScatter);
 vec3 direct=cloudRow(10).rgb*(directTransmission*(0.32+phase*0.23)*silver+multipleScatter*0.60);
 return max(ambient+direct,vec3(0.0));
}
vec2 cloudSphere(vec3 ro,vec3 rd,float radius){
 float b=dot(ro,rd);float c=dot(ro,ro)-radius*radius;float d=b*b-c;
 if(d<0.0)return vec2(1.0,-1.0);float root=sqrt(d);return vec2(-b-root,-b+root);
}
vec2 cloudInterval(vec3 ro,vec3 rd,int layer){
 vec4 profile=cloudRow(6+layer);float radius=cloudRow(3).w;
 vec2 outer=cloudSphere(ro,rd,6371000.0+profile.x+profile.y);
 float lo=max(0.0,outer.x);float hi=min(radius,outer.y);
 if(hi<=lo)return vec2(1.0,-1.0);
 vec2 inner=cloudSphere(ro,rd,6371000.0+profile.x);
 if(inner.y>inner.x){if(lo<inner.x&&inner.x>0.0){hi=min(hi,inner.x);}else if(lo<inner.y){lo=max(lo,inner.y);}}
 vec2 ground=cloudSphere(ro,rd,6371000.0);
 if(ground.y>ground.x&&ground.x>=0.0)hi=min(hi,ground.x);
 return vec2(lo,hi);
}
vec4 cloudComposite(vec3 localRay,vec4 background){
 if(cloudRow(0).w<0.5)return background;
 vec3 ro=cloudRow(0).xyz;
 vec3 rd=normalize(cloudRow(1).xyz*localRay.x+cloudRow(2).xyz*localRay.y+cloudRow(3).xyz*localRay.z);
 vec3 sun=normalize(cloudRow(9).xyz);
 vec3 radiance=vec3(0.0);float transmittance=1.0;int used=0;
 for(int layerPass=0;layerPass<3;layerPass++){
  int chosen=-1;float nearest=100000000.0;
  for(int layer=0;layer<3;layer++){
   if((used&(1<<layer))!=0)continue;
   vec2 interval=cloudInterval(ro,rd,layer);
   if(interval.y>interval.x&&interval.x<nearest){chosen=layer;nearest=interval.x;}
  }
  if(chosen<0)break;used|=1<<chosen;
  vec2 interval=cloudInterval(ro,rd,chosen);float stride=(interval.y-interval.x)/24.0;
  // Stable sub-step sampling avoids a frame-random grain over the authored sky.
  for(int step=0;step<24;step++){
   if(transmittance<0.015)break;
   float t=interval.x+(float(step)+0.5)*stride;vec3 p=ro+rd*t;
   float fade=1.0-smoothstep(cloudRow(3).w*0.80,cloudRow(3).w,t);
   float density=cloudLayerDensity(p,chosen,true,stride)*fade;
   if(density>0.000015){float alpha=1.0-exp(-density*stride*0.75);radiance+=cloudLight(p,rd,sun,density)*(transmittance*alpha);transmittance*=1.0-alpha;}
  }
 }
 return vec4(radiance+background.rgb*transmittance,1.0);
}
// END GENERATED CLOUDS
// BEGIN GENERATED EXPANSE SKY
// Distant, fictional blue-white sky. Direction-space noise has no UV seam or
// camera translation. No cloud/weather data or scene lighting is changed here.
float celestialHash(vec3 p) {
 return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453);
}
float celestialNoise(vec3 p) {
 vec3 cell=floor(p);
 vec3 f=fract(p);
 f=f*f*(vec3(3.0)-2.0*f);
 return mix(mix(mix(celestialHash(cell),celestialHash(cell+vec3(1.0,0.0,0.0)),f.x),mix(celestialHash(cell+vec3(0.0,1.0,0.0)),celestialHash(cell+vec3(1.0,1.0,0.0)),f.x),f.y),mix(mix(celestialHash(cell+vec3(0.0,0.0,1.0)),celestialHash(cell+vec3(1.0,0.0,1.0)),f.x),mix(celestialHash(cell+vec3(0.0,1.0,1.0)),celestialHash(cell+vec3(1.0,1.0,1.0)),f.x),f.y),f.z);
}
float celestialFilaments(vec3 p) {
 return celestialNoise(p)*0.55+celestialNoise(p*2.03+vec3(13.1))*0.28+celestialNoise(p*4.11+vec3(7.7))*0.12+celestialNoise(p*8.21)*0.05;
}
vec3 celestialStars(vec3 ray,float scale,float occupancy,float gain) {
 // Centers stay inside their cells, so neighboring cells cannot clip a star.
 vec3 p=ray*scale;
 vec3 cell=floor(p);
 float seed=celestialHash(cell);
 vec3 center=vec3(celestialHash(cell+vec3(17.0)),celestialHash(cell+vec3(43.0)),celestialHash(cell+vec3(91.0)))*0.5+vec3(0.25);
 float distance=length(fract(p)-center);
 float radius=mix(0.055,0.14,pow(seed,12.0));
 // Preserve point energy as a star becomes smaller than a render pixel.
 float footprint=length(fwidth(p))*0.5;
 float widthSquared=radius*radius+footprint*footprint;
 float core=exp(-distance*distance/widthSquared)*radius*radius/widthSquared;
 float halo=exp(-distance*distance/(radius*radius*5.0))*0.07;
 float present=step(1.0-occupancy,celestialHash(cell+vec3(113.0)));
 vec3 tint=mix(vec3(0.55,0.72,1.0),vec3(0.94,0.97,1.0),seed);
 return tint*(core+halo)*present*gain;
}
vec3 celestialSky(vec3 ray,float hour) {
 // One revolution per day; wrapping is continuous at midnight. This is an
 // authored celestial sphere, not an Earth catalog or a foreground galaxy.
 float angle=hour*0.2617993878;
 vec3 pole=normalize(vec3(0.18,0.83,0.53));
 vec3 sky=ray*cos(angle)+cross(pole,ray)*sin(angle)+pole*dot(pole,ray)*(1.0-cos(angle));
 float latitude=dot(sky,normalize(vec3(0.34,0.58,-0.74)));
 float broad=exp(-latitude*latitude*18.0);
 float ribbon=exp(-latitude*latitude*95.0);
 float filaments=celestialFilaments(sky*8.0+vec3(31.0));
 float dust=smoothstep(0.38,0.64,celestialFilaments(sky*15.0+vec3(71.0)));
 float structure=smoothstep(0.24,0.76,filaments);
 vec3 glow=vec3(0.055,0.095,0.18)*broad*structure;
 glow+=vec3(0.11,0.16,0.23)*ribbon*structure*(1.0-dust*0.88);
 float wisp=pow(max(0.0,celestialFilaments(sky*5.0+vec3(103.0))-0.38),2.0);
 glow+=vec3(0.10,0.23,0.38)*wisp*broad;
 vec3 stars=celestialStars(sky,150.0,0.30,0.68);
 stars+=celestialStars(sky+vec3(7.0),310.0,0.38,0.30);
 stars+=celestialStars(sky+vec3(19.0),95.0,0.07,1.65);
 stars+=celestialStars(sky+vec3(29.0),420.0,0.52,0.20)*broad;
 return glow+stars*(1.0-dust*ribbon*0.65);
}
vec3 skyProfile0(float z){
 if(z<=0.5){return mix(vec3(0.007843137255,0.02745098039,0.09803921569),vec3(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.02745098039,0.06666666667,0.1529411765),vec3(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3(0.06274509804,0.1254901961,0.2470588235);
}
vec3 skyProfile1(float z){
 if(z<=0.5){return mix(vec3(0.007843137255,0.02745098039,0.09803921569),vec3(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.02745098039,0.06666666667,0.1529411765),vec3(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3(0.06274509804,0.1254901961,0.2470588235);
}
vec3 skyProfile2(float z){
 if(z<=0.5){return mix(vec3(0.01960784314,0.05098039216,0.1647058824),vec3(0.06274509804,0.137254902,0.2901960784),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.06274509804,0.137254902,0.2901960784),vec3(0.1450980392,0.2705882353,0.431372549),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3(0.1450980392,0.2705882353,0.431372549);
}
vec3 skyProfile3(float z){
 if(z<=0.2){return mix(vec3(0.003921568627,0.03921568627,0.2117647059),vec3(0.05098039216,0.1294117647,0.3607843137),clamp((z-0.0)/0.2,0.0,1.0));}
 if(z<=0.45){return mix(vec3(0.05098039216,0.1294117647,0.3607843137),vec3(0.1568627451,0.2588235294,0.5803921569),clamp((z-0.2)/0.25,0.0,1.0));}
 if(z<=0.65){return mix(vec3(0.1568627451,0.2588235294,0.5803921569),vec3(0.2392156863,0.3725490196,0.7254901961),clamp((z-0.45)/0.2,0.0,1.0));}
 if(z<=0.85){return mix(vec3(0.2392156863,0.3725490196,0.7254901961),vec3(0.3607843137,0.5176470588,0.8196078431),clamp((z-0.65)/0.2,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.3607843137,0.5176470588,0.8196078431),vec3(0.4823529412,0.6549019608,0.8862745098),clamp((z-0.85)/0.15,0.0,1.0));}
 return vec3(0.4823529412,0.6549019608,0.8862745098);
}
vec3 skyProfile4(float z){
 if(z<=0.45){return mix(vec3(0.02352941176,0.1529411765,0.3607843137),vec3(0.0431372549,0.2,0.4),clamp((z-0.0)/0.45,0.0,1.0));}
 if(z<=0.75){return mix(vec3(0.0431372549,0.2,0.4),vec3(0.09019607843,0.2470588235,0.4470588235),clamp((z-0.45)/0.3,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.09019607843,0.2470588235,0.4470588235),vec3(0.1764705882,0.3294117647,0.5529411765),clamp((z-0.75)/0.25,0.0,1.0));}
 return vec3(0.1764705882,0.3294117647,0.5529411765);
}
vec3 skyProfile5(float z){
 if(z<=0.35){return mix(vec3(0.3490196078,0.4549019608,0.6392156863),vec3(0.3333333333,0.4392156863,0.6235294118),clamp((z-0.0)/0.35,0.0,1.0));}
 if(z<=0.7){return mix(vec3(0.3333333333,0.4392156863,0.6235294118),vec3(0.2980392157,0.4156862745,0.6117647059),clamp((z-0.35)/0.35,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.2980392157,0.4156862745,0.6117647059),vec3(0.3294117647,0.4745098039,0.6901960784),clamp((z-0.7)/0.3,0.0,1.0));}
 return vec3(0.3294117647,0.4745098039,0.6901960784);
}
vec3 skyProfile6(float z){
 if(z<=0.35){return mix(vec3(0.3490196078,0.4549019608,0.6392156863),vec3(0.3333333333,0.4392156863,0.6235294118),clamp((z-0.0)/0.35,0.0,1.0));}
 if(z<=0.7){return mix(vec3(0.3333333333,0.4392156863,0.6235294118),vec3(0.2980392157,0.4156862745,0.6117647059),clamp((z-0.35)/0.35,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.2980392157,0.4156862745,0.6117647059),vec3(0.3294117647,0.4745098039,0.6901960784),clamp((z-0.7)/0.3,0.0,1.0));}
 return vec3(0.3294117647,0.4745098039,0.6901960784);
}
vec3 skyProfile7(float z){
 if(z<=0.45){return mix(vec3(0.3882352941,0.5137254902,0.6705882353),vec3(0.3960784314,0.5215686275,0.6745098039),clamp((z-0.0)/0.45,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.3960784314,0.5215686275,0.6745098039),vec3(0.5294117647,0.6274509804,0.7490196078),clamp((z-0.45)/0.55,0.0,1.0));}
 return vec3(0.5294117647,0.6274509804,0.7490196078);
}
vec3 skyProfile8(float z){
 if(z<=0.45){return mix(vec3(0.02352941176,0.1529411765,0.3607843137),vec3(0.0431372549,0.2,0.4),clamp((z-0.0)/0.45,0.0,1.0));}
 if(z<=0.75){return mix(vec3(0.0431372549,0.2,0.4),vec3(0.09019607843,0.2470588235,0.4470588235),clamp((z-0.45)/0.3,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.09019607843,0.2470588235,0.4470588235),vec3(0.1764705882,0.3294117647,0.5529411765),clamp((z-0.75)/0.25,0.0,1.0));}
 return vec3(0.1764705882,0.3294117647,0.5529411765);
}
vec3 skyProfile9(float z){
 if(z<=0.2){return mix(vec3(0.003921568627,0.03921568627,0.2117647059),vec3(0.05098039216,0.1294117647,0.3607843137),clamp((z-0.0)/0.2,0.0,1.0));}
 if(z<=0.45){return mix(vec3(0.05098039216,0.1294117647,0.3607843137),vec3(0.1568627451,0.2588235294,0.5803921569),clamp((z-0.2)/0.25,0.0,1.0));}
 if(z<=0.65){return mix(vec3(0.1568627451,0.2588235294,0.5803921569),vec3(0.2392156863,0.3725490196,0.7254901961),clamp((z-0.45)/0.2,0.0,1.0));}
 if(z<=0.85){return mix(vec3(0.2392156863,0.3725490196,0.7254901961),vec3(0.3607843137,0.5176470588,0.8196078431),clamp((z-0.65)/0.2,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.3607843137,0.5176470588,0.8196078431),vec3(0.4823529412,0.6549019608,0.8862745098),clamp((z-0.85)/0.15,0.0,1.0));}
 return vec3(0.4823529412,0.6549019608,0.8862745098);
}
vec3 skyProfile10(float z){
 if(z<=0.5){return mix(vec3(0.007843137255,0.03137254902,0.1176470588),vec3(0.03921568627,0.09411764706,0.2235294118),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.03921568627,0.09411764706,0.2235294118),vec3(0.09411764706,0.1921568627,0.3529411765),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3(0.09411764706,0.1921568627,0.3529411765);
}
vec3 skyProfile11(float z){
 if(z<=0.5){return mix(vec3(0.007843137255,0.02745098039,0.09803921569),vec3(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.02745098039,0.06666666667,0.1529411765),vec3(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3(0.06274509804,0.1254901961,0.2470588235);
}
vec3 skyProfile12(float z){
 if(z<=0.5){return mix(vec3(0.007843137255,0.02745098039,0.09803921569),vec3(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3(0.02745098039,0.06666666667,0.1529411765),vec3(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3(0.06274509804,0.1254901961,0.2470588235);
}
vec3 skyPalette(float minute,float z){
 if(minute<=240.0){return mix(skyProfile0(z),skyProfile1(z),clamp((minute-0.0)/240.0,0.0,1.0));}
 if(minute<=300.0){return mix(skyProfile1(z),skyProfile2(z),clamp((minute-240.0)/60.0,0.0,1.0));}
 if(minute<=345.0){return mix(skyProfile2(z),skyProfile3(z),clamp((minute-300.0)/45.0,0.0,1.0));}
 if(minute<=390.0){return mix(skyProfile3(z),skyProfile4(z),clamp((minute-345.0)/45.0,0.0,1.0));}
 if(minute<=480.0){return mix(skyProfile4(z),skyProfile5(z),clamp((minute-390.0)/90.0,0.0,1.0));}
 if(minute<=660.0){return mix(skyProfile5(z),skyProfile6(z),clamp((minute-480.0)/180.0,0.0,1.0));}
 if(minute<=900.0){return mix(skyProfile6(z),skyProfile7(z),clamp((minute-660.0)/240.0,0.0,1.0));}
 if(minute<=1020.0){return mix(skyProfile7(z),skyProfile8(z),clamp((minute-900.0)/120.0,0.0,1.0));}
 if(minute<=1110.0){return mix(skyProfile8(z),skyProfile9(z),clamp((minute-1020.0)/90.0,0.0,1.0));}
 if(minute<=1170.0){return mix(skyProfile9(z),skyProfile10(z),clamp((minute-1110.0)/60.0,0.0,1.0));}
 if(minute<=1260.0){return mix(skyProfile10(z),skyProfile11(z),clamp((minute-1170.0)/90.0,0.0,1.0));}
 if(minute<=1440.0){return mix(skyProfile11(z),skyProfile12(z),clamp((minute-1260.0)/180.0,0.0,1.0));}
 return skyProfile0(z);
}
vec4 expanseSky(vec3 direction,float kind){
 vec3 ray=normalize(direction);
 float hour=fract(-kind-10.0)*24.0;
 float angle=(hour-6.0)/12.0*3.14159265359;
 vec3 sun=vec3(0.0,sin(angle),-cos(angle));
 float elevation=asin(clamp(ray.y,0.0,1.0))*57.295779513;
 vec3 color=skyPalette(hour*60.0,1.0-elevation/90.0);
 float sunAltitude=(18.0-hour)*15.0;
 float azimuthCos=clamp(dot(sun.xz,ray.xz)/max(length(sun.xz)*length(ray.xz),0.00001),-1.0,1.0);
 float toward=pow((1.0+azimuthCos)*0.5,0.75);
 float opposite=pow((1.0-azimuthCos)*0.5,1.25);
 float strength=smoothstep(15.0,17.5,hour)*(1.0-smoothstep(18.0,19.3,hour));
 float shadowHeight=max(0.0,-sunAltitude)*1.65;
 float shadow=smoothstep(0.0,5.0,-sunAltitude)*opposite*(1.0-smoothstep(shadowHeight,shadowHeight+12.0,elevation))*(1.0-smoothstep(18.75,19.5,hour));
 color=mix(color,vec3(0.025,0.062,0.16),shadow*0.72);
 float bandHeight=12.0+max(0.0,sunAltitude)*0.55;
 float warm=strength*toward*exp(-pow(elevation/bandHeight,1.45));
 color=mix(color,vec3(0.72,0.8,0.86),smoothstep(0.0,0.45,warm)*0.82);
 color=mix(color,vec3(0.95,0.95,0.8),smoothstep(0.18,0.66,warm));
 color=mix(color,vec3(1.0,0.9,0.32),smoothstep(0.48,1.0,warm));
 // Distant celestial light is behind the original sun/moon and cloud pass.
 float night=1.0-smoothstep(-0.38,-0.08,sun.y);
 color+=celestialSky(ray,hour)*night*smoothstep(-0.02,0.22,ray.y);
 // Angular disks are an adaptation of the original billboard celestials.
 float sunDisk=smoothstep(cos(0.0048),cos(0.0043),dot(ray,sun));
 float moonDisk=smoothstep(cos(0.0048),cos(0.0043),dot(ray,-sun));
 color=mix(color,vec3(1.0,1.0,0.8),sunDisk);
 color=mix(color,vec3(205.0,215.0,233.0)/255.0,moonDisk);
 return vec4(color,1.0);
}
// END GENERATED EXPANSE SKY
void main(){
 // Incense smoke material matches WebGPU soft coverage and ordinary scene depth.
 if(material==121.){float coverage=data.z*pow(max(0.,1.-dot(data.xy,data.xy)),2.);if(coverage<0.001)discard;pixel=vec4(.63,.66,.67,coverage);return;}
float aspect=abs(dFdy(screen.y))/max(abs(dFdx(screen.x)),0.000001);if(material<=-4096.){pixel=surface(data,material);}else if(material<=-9.5){pixel=cloudComposite(data.xyz,expanseSky(data.xyz,material));}else if(material<=-3.5){pixel=colorEmotion(data,material,screen,aspect);if(pixel.a<=0.001)discard;}else{pixel=surface(data,material);}}
