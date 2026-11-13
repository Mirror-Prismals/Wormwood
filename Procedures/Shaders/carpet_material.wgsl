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
