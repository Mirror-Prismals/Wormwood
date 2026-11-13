fn theatreTexel(tile:u32, p:vec2<i32>)->vec4<f32>{let xy=vec2<u32>(p & vec2<i32>(127)); let n=tile*16384u+xy.y*128u+xy.x; let row=arrayLength(&workstationData.rows)-20480u+n/4u; return unpack4x8unorm(bitcast<u32>(gemRow(i32(row))[n%4u]));}
fn theatreSample(d:vec4<f32>)->vec4<f32>{
 let p=fract(d.xy)*128.0-0.5; let i=vec2<i32>(floor(p)); let f=fract(p);
 let tile=select(u32(round(d.w))-100u,4u,d.w==114.);
 let c=mix(mix(theatreTexel(tile,i), theatreTexel(tile,i+vec2<i32>(1,0)),f.x),
           mix(theatreTexel(tile,i+vec2<i32>(0,1)),theatreTexel(tile,i+vec2<i32>(1,1)),f.x),f.y);
 return vec4<f32>(mix(vec3<f32>(0.22,0.025,0.04),c.rgb,c.a)*d.z*vec3<f32>(1.08,1.0,0.9),1.0);
}

fn tableRead(n:u32)->vec4<f32>{let b=arrayLength(&workstationData.rows)-20480u-1135961u;return unpack4x8unorm(bitcast<u32>(gemRow(i32(b+n/4u))[n%4u]));}
fn tableEyeValue()->vec3<f32>{return gemRow(i32(arrayLength(&workstationData.rows)-20480u-1135961u)).xyz;}
fn tableTexel(map:i32,level:i32,p:vec2<i32>)->vec4<f32>{let size=512>>u32(level);let xy=vec2<u32>(p & vec2<i32>(size-1));let offset=u32((262144-size*size)*4/3);return tableRead(4u+u32(map)*349525u+offset+xy.y*u32(size)+xy.x);}
fn tableMap(map:i32,uv:vec2<f32>,lod:f32)->vec4<f32>{
 let level=i32(clamp(floor(lod),0.,9.));let size=512>>u32(level);
 let p=fract(uv)*f32(size)-.5;let i=vec2<i32>(floor(p));let f=fract(p);
 return mix(mix(tableTexel(map,level,i),tableTexel(map,level,i+vec2<i32>(1,0)),f.x),mix(tableTexel(map,level,i+vec2<i32>(0,1)),tableTexel(map,level,i+vec2<i32>(1,1)),f.x),f.y);
}
fn tableSurface(d:vec4<f32>,footprint:f32)->vec4<f32>{
 let face=i32(round(d.w))-106;
 let ns=array<vec3<f32>,6>(vec3<f32>(0,1,0),vec3<f32>(0,-1,0),vec3<f32>(0,0,1),vec3<f32>(0,0,-1),vec3<f32>(1,0,0),vec3<f32>(-1,0,0));
 let ts=array<vec3<f32>,6>(vec3<f32>(1,0,0),vec3<f32>(1,0,0),vec3<f32>(1,0,0),vec3<f32>(-1,0,0),vec3<f32>(0,0,-1),vec3<f32>(0,0,1));
 let N=ns[face];let T=ts[face];let B=cross(N,T);
 let p=T*d.x*2.+B*d.y*.65+N*d.z;
 let V=normalize(tableEyeValue()-p);let vt=vec3<f32>(dot(V,T),dot(V,B),dot(V,N));
 let lod=clamp(log2(max(footprint*512.,1.)),0.,9.);
 var uv=d.xy;let travel=vt.xy/max(abs(vt.z),.2)*vec2<f32>(.0004/2.,.0004/.65);
 for(var step=0;step<4;step++){let h=tableMap(2,uv,lod).g-.5;uv=d.xy+travel*h;}
 let packed=tableMap(2,uv,lod);let rough=clamp(packed.r,.24,.85);
 let nm=normalize(tableMap(1,uv,lod).rgb*2.-1.);let n=normalize(T*nm.x+B*nm.y+N*nm.z);
 let albedo=pow(tableMap(0,uv,lod).rgb,vec3<f32>(2.2));var color=albedo*.30*packed.a;
 let lights=array<vec3<f32>,5>(vec3<f32>(-23,15,-24),vec3<f32>(23,15,-24),vec3<f32>(-18,15,18),vec3<f32>(18,15,18),vec3<f32>(0,13,-49));
 for(var j=0;j<5;j++){
  let delta=lights[j]-p;let L=normalize(delta);let H=normalize(L+V);
  let nl=max(dot(n,L),0.);let nv=max(dot(n,V),.02);let nh=max(dot(n,H),0.);let vh=max(dot(V,H),0.);
  let a=rough*rough;let a2=a*a;let denom=nh*nh*(a2-1.)+1.;let D=a2/(3.14159265*denom*denom);
  let k=(rough+1.)*(rough+1.)/8.;let G=(nl/(nl*(1.-k)+k))*(nv/(nv*(1.-k)+k));
  let F=vec3<f32>(.04)+vec3<f32>(.96)*pow(1.-vh,5.);let spec=D*G*F/max(4.*nl*nv,.001);
  let radiance=vec3<f32>(1.,.86,.68)*2.2/(1.+dot(delta,delta)/180.);
  color+=((vec3<f32>(1.)-F)*albedo/3.14159265+spec)*radiance*nl;
 }
 return vec4<f32>(pow(clamp(color,vec3<f32>(0),vec3<f32>(1)),vec3<f32>(1./2.2)),1.);
}
// BEGIN EXTERIOR LIMESTONE
fn exteriorFrame(row:i32)->vec4<f32>{var v:vec4<f32>;for(var j=0;j<4;j++){v[j]=bitcast<f32>(pack4x8unorm(tableRead(u32(2446684+row*4+j))));}return v;}

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
// END EXTERIOR LIMESTONE
// BEGIN RED CARPET
// Metric red wool; immutable offline maps, shallow relief, five theatre lamps.
fn carpetLevel(map:i32,uv:vec2<f32>,level:i32)->vec4<f32>{
 let size=512>>u32(level);let p=fract(uv)*f32(size)-.5;let ip=vec2<i32>(floor(p));let f=fract(p);
 let offset=3495268+map*349525+(262144-size*size)*4/3;
 let a=ip&vec2<i32>(size-1);let b=(ip+vec2<i32>(1,0))&vec2<i32>(size-1);
 let c=(ip+vec2<i32>(0,1))&vec2<i32>(size-1);let d=(ip+vec2<i32>(1,1))&vec2<i32>(size-1);
 return mix(mix(tableRead(u32(offset+a.y*size+a.x)),tableRead(u32(offset+b.y*size+b.x)),f.x),mix(tableRead(u32(offset+c.y*size+c.x)),tableRead(u32(offset+d.y*size+d.x)),f.x),f.y);
}
fn carpetMap(map:i32,uv:vec2<f32>,lod:f32)->vec4<f32>{
 let lo=i32(floor(lod));return mix(carpetLevel(map,uv,lo),carpetLevel(map,uv,min(lo+1,9)),fract(lod));
}
fn carpetSurface(data:vec4<f32>,footprint:f32)->vec4<f32>{
 let p=data.xyz;var slope=0.;if data.w==118.{slope=5.5/66.;}else if data.w==119.{slope=5./12.;}else if data.w==120.{slope=-6./17.;}
 let N=normalize(vec3<f32>(0,1,slope));let T=vec3<f32>(1,0,0);let B=cross(N,T);
 let V=normalize(tableEyeValue()-p);let vt=vec3<f32>(dot(V,T),dot(V,B),dot(V,N));
 let baseUV=vec2<f32>(dot(p,T),dot(p,B))/.256;
 let lod=clamp(log2(max(footprint*512./.256,1.)),0.,9.);
 var uv=baseUV;let travel=vt.xy/max(abs(vt.z),.2)*(.0012/.256);
 for(var j=0;j<4;j++){uv=baseUV+travel*(carpetMap(2,uv,lod).g-.5);}
 let packed=carpetMap(2,uv,lod);let rough=clamp(packed.r,.86,.99);
 let nm=normalize(carpetMap(1,uv,lod).rgb*2.-1.);let n=normalize(T*nm.x+B*nm.y+N*nm.z);
 let albedo=pow(carpetMap(0,uv,lod).rgb,vec3<f32>(2.2));var color=albedo*.30*packed.a;
 let lamps=array<vec3<f32>,5>(vec3<f32>(-23,15,-24),vec3<f32>(23,15,-24),vec3<f32>(-18,15,18),vec3<f32>(18,15,18),vec3<f32>(0,13,-49));
 for(var j=0;j<5;j++){
  let delta=lamps[j]-p;let L=normalize(delta);let H=normalize(L+V);
  let nl=max(dot(n,L),0.);let nv=max(dot(n,V),.02);let nh=max(dot(n,H),0.);let vh=max(dot(V,H),0.);
  let a=rough*rough;let a2=a*a;let denom=nh*nh*(a2-1.)+1.;let D=a2/(3.14159265*denom*denom);
  let k=(rough+1.)*(rough+1.)/8.;let G=nl/(nl*(1.-k)+k)*nv/(nv*(1.-k)+k);
  let F=vec3<f32>(.025)+vec3<f32>(.975)*pow(1.-vh,5.);let spec=D*G*F/max(4.*nl*nv,.001);
  // A restrained red-tinted grazing sheen approximates the Blender wool lobe.
  let sheen=albedo*.08*pow(1.-vh,5.);
  let radiance=vec3<f32>(1,.86,.68)*2.2/(1.+dot(delta,delta)/180.);
  color+=((vec3<f32>(1.)-F)*albedo/3.14159265+spec+sheen)*radiance*nl;
 }
 return vec4<f32>(pow(clamp(color,vec3<f32>(0),vec3<f32>(1)),vec3<f32>(1./2.2)),1.);
}
// END RED CARPET
