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
