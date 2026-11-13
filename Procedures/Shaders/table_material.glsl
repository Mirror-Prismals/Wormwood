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
