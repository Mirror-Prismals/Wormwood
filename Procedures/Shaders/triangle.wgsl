struct SceneVertex {position:vec4<f32>,data:vec4<f32>};
struct Appearance {vertices:array<SceneVertex>};
@group(0) @binding(0) var<storage,read> appearance:Appearance;
struct PlayerTexture {width:u32,height:u32,pixels:array<u32>};
@group(0) @binding(1) var<storage,read> playerTexture:PlayerTexture;
struct VertexOutput {@builtin(position) position:vec4<f32>,@location(0) data:vec4<f32>,@location(1) @interpolate(flat) material:f32,@location(3) viewDistance:f32,@location(4) soilUV:vec2<f32>,@location(2) screen:vec2<f32>};
@vertex fn vs_main(@builtin(vertex_index) index:u32)->VertexOutput {var o:VertexOutput;o.position=appearance.vertices[index].position;o.screen=o.position.xy/o.position.w;o.viewDistance=o.position.w;o.data=appearance.vertices[index].data;o.material=o.data.w;let code=max(o.data.w-4096.0,0.0);o.soilUV=vec2<f32>(code%2048.0,floor(code/2048.0))/128.0;if o.data.w<=-4096.{var rockCode=-o.data.w-4096.;let tag=floor(rockCode/1048576.);rockCode=rockCode%1048576.;o.soilUV=vec2<f32>(rockCode%1024.,floor(rockCode/1024.))/64.;o.material=-4096.-tag;}return o;}
fn texel(p:vec2<i32>)->vec4<f32>{let i=clamp(p,vec2<i32>(0),vec2<i32>(i32(playerTexture.width),i32(playerTexture.height))-vec2<i32>(1));return unpack4x8unorm(playerTexture.pixels[u32(i.y)*playerTexture.width+u32(i.x)]);}
fn playerSample(uv:vec2<f32>)->vec4<f32>{let p=uv*vec2<f32>(f32(playerTexture.width),f32(playerTexture.height))-vec2<f32>(0.5);let i=vec2<i32>(floor(p));let f=fract(p);return mix(mix(texel(i),texel(i+vec2<i32>(1,0)),f.x),mix(texel(i+vec2<i32>(0,1)),texel(i+vec2<i32>(1,1)),f.x),f.y);}
fn colorEmotion(effect:vec4<f32>,kind:f32,screen:vec2<f32>,aspect:f32)->vec4<f32>{
 let p=vec2<f32>(screen.x*aspect,screen.y*1.55);let r=length(p);let edge=pow(smoothstep(1.48,2.10,r),0.85);let pulse=0.92+0.08*sin(effect.y*2.2+r*8.0);let charge=clamp(effect.z,0.0,1.0);
 let a=select(vec3<f32>(0.10,0.85,0.20),vec3<f32>(1.0,0.55,0.10),kind < -4.5);let b=select(vec3<f32>(1.0,0.0,1.0),vec3<f32>(0.10,0.85,0.20),kind < -4.5);var color=mix(a,b,charge);
 if charge>=0.999{color=mix(b,a,smoothstep(-0.01,0.01,cos(atan2(p.y,p.x)-effect.y*3.0)));}return vec4<f32>(color,clamp(edge*pulse*effect.x,0.0,0.98)*0.10);
}
fn soilNoise(p:vec2<f32>,period:f32)->f32 {
 let i=floor(p);let f=fract(p);let t=f*f*(3.0-2.0*f);
 var h=(vec4<f32>(i.x,i.x+1.0,i.x,i.x+1.0)%vec4<f32>(period))*73.0+
       (vec4<f32>(i.y,i.y,i.y+1.0,i.y+1.0)%vec4<f32>(period))*151.0;
 h=fract(h*0.1031);h=fract(h*(h+33.33)*(h+h));
 return mix(mix(h.x,h.y,t.x),mix(h.z,h.w,t.x),t.y)-0.5;
}
fn soilDiffuse(rgb:vec3<f32>,uv:vec2<f32>,fade:f32,footprint:f32)->vec3<f32> {
 let grass=smoothstep(0.02,0.10,rgb.g-rgb.r);
 if grass>0.5{return grassDiffuse(rgb,uv,footprint);}
 if rgb.r>rgb.g*1.28 && rgb.b<rgb.g*.85{return dirtDiffuse(rgb,uv,footprint);}
 let mottling=soilNoise(uv*4.0,16.0);let grains=soilNoise(uv*32.0,128.0);
 let blades=soilNoise(vec2<f32>(uv.x*48.0+uv.y*24.0,uv.y*24.0),96.0);
 return rgb*(1.0+0.42*mottling+fade*(0.34*grains+0.48*grass*blades))
       +grass*vec3<f32>(0.043,0.034,0.008)*max(0.0,mottling+blades)*fade;
}
// BEGIN GENERATED CLOUDS
fn cloudNoiseFetch(point:vec2<i32>)->vec4<f32>{let p=clamp(point,vec2<i32>(0),vec2<i32>(527,722));return unpack4x8unorm(playerTexture.pixels[4221956u+u32(p.y*528+p.x)]);}
fn cloudRow(i:i32)->vec4<f32>{let k=4603702u+u32(i)*4u;return vec4<f32>(bitcast<f32>(playerTexture.pixels[k]),bitcast<f32>(playerTexture.pixels[k+1u]),bitcast<f32>(playerTexture.pixels[k+2u]),bitcast<f32>(playerTexture.pixels[k+3u]));}
// Canonical cloud equations, translated into each native scene fragment.
// Noise/mip sampling, layer shapes and scattering follow the original engine.
// Spherical intervals and authored initial coverage are Wormwood adaptations.
fn cloudPick(a:f32,b:f32,condition:bool)->f32 {if(condition){return b;}return a;}
fn cloudOffset(level:i32)->f32 {if(level==0){return 0.0;}if(level==1){return 528.0;}if(level==2){return 664.0;}if(level==3){return 700.0;}if(level==4){return 710.0;}if(level==5){return 716.0;}return 720.0;}
fn cloudBilinear(point:vec2<f32>)->vec4<f32> {
 var p:vec2<f32>=point-vec2<f32>(0.5);var i:vec2<i32>=vec2<i32>(floor(p));var f:vec2<f32>=fract(p);
 return mix(mix(cloudNoiseFetch(i),cloudNoiseFetch(i+vec2<i32>(1,0)),f.x),mix(cloudNoiseFetch(i+vec2<i32>(0,1)),cloudNoiseFetch(i+vec2<i32>(1,1)),f.x),f.y);
}
fn cloudSlice(uv:vec2<f32>,slice:f32,level:i32)->vec4<f32> {
 var side:f32=f32(64>>u32(level));var tileSize:f32=side+2.0;var columns:f32=min(8.0,side);
 var index:f32=slice-floor(slice/side)*side;
 var tile:vec2<f32>=vec2<f32>(index-floor(index/columns)*columns,floor(index/columns));
 return cloudBilinear(tile*tileSize+vec2<f32>(1.0,cloudOffset(level)+1.0)+fract(uv)*side);
}
fn cloudVolumeLevel(p:vec3<f32>,level:i32)->vec4<f32> {
 var q:vec3<f32>=fract(p);var side:f32=f32(64>>u32(level));var z:f32=q.z*side-0.5;var lo:f32=floor(z);
 return mix(cloudSlice(q.xy,lo,level),cloudSlice(q.xy,lo+1.0,level),fract(z));
}
fn cloudVolume(p:vec3<f32>,lod:f32)->vec4<f32> {
 var mip:f32=clamp(lod,0.0,6.0);var lo:i32=i32(floor(mip));var f:f32=fract(mip);
 var low:vec4<f32>=cloudVolumeLevel(p,lo);if(f<0.02||lo==6){return low;}
 return mix(low,cloudVolumeLevel(p,min(lo+1,6)),f);
}
fn cloudLayerDensity(world:vec3<f32>,layer:i32,detailed:bool,footprint:f32)->f32 {
 var profile:vec4<f32>=cloudRow(6+layer);var coverage:f32=profile.z;var convection:f32=profile.w;
 if(coverage<0.008){return 0.0;}
 var altitude:f32=length(world)-6371000.0;
 var height:f32=(altitude-profile.x)/max(profile.y,1.0);
 if(height<=0.0||height>=1.0){return 0.0;}
 var wind:vec4<f32>=cloudRow(4);var upper:vec4<f32>=cloudRow(5);var evolution:f32=wind.z/512.0;
 var continuousLod:f32=log2(max(1.0,footprint/64.0));var lod:f32=cloudPick(round(continuousLod),continuousLod,detailed);
 var p:vec3<f32>=vec3<f32>(world.x,altitude,world.z);
 var q:vec3<f32>=vec3<f32>(p.x-wind.x,p.y-profile.x,p.z-wind.y)/4096.0;
 var shape:f32=0.0;var vertical:f32=0.0;var optical:f32=1.0;
 if(layer==0){
  q.y=q.y*mix(0.30,0.85,convection);
  var anvil:f32=smoothstep(0.68,0.95,height)*smoothstep(0.65,1.0,convection);
  q.x=q.x*(1.0-anvil*0.30);q.z=q.z*(1.0-anvil*0.30);
  var column:f32=cloudVolume(vec3<f32>(q.x*0.5,0.37,q.z*0.5),max(0.0,lod-1.0)).a;
  var columnMask:f32=smoothstep(mix(0.72,0.24,coverage),mix(0.84,0.40,coverage),column);
  if(columnMask<0.001&&coverage<0.78){return 0.0;}
  var warp:vec4<f32>=vec4<f32>(0.5);
  if(detailed){warp=cloudVolume(q*0.5+vec3<f32>(0.0,evolution*0.25,evolution*0.5),max(0.0,lod-1.0));}
  var n:vec4<f32>=cloudVolume(q+(warp.gba-vec3<f32>(0.5))*0.14,lod);
  var towerTop:f32=mix(0.40,1.0,smoothstep(0.30,0.70,column));
  var threshold:f32=mix(0.76,0.18,coverage)+convection*pow(height/towerTop,2.0)*(0.20-anvil*0.15);
  shape=smoothstep(threshold-0.075,threshold+0.13,n.r)*columnMask;
  var sheet:f32=smoothstep(0.78,0.98,coverage)*(1.0-convection);
  shape=mix(shape,mix(0.70,1.0,n.a),sheet*0.65);
  vertical=smoothstep(0.0,mix(0.08,0.16,convection),height)*(1.0-smoothstep(mix(0.60,0.80,convection),1.0,height));
  if(detailed&&shape>0.001){
   var erosion:f32=(1.0-n.g)*0.65+(1.0-n.b)*0.35;
   var edge:f32=1.0-smoothstep(0.05,0.80,shape);var detailFade:f32=1.0-smoothstep(80.0,350.0,footprint);
   shape=clamp(shape-erosion*edge*mix(0.18,0.62,convection)*wind.w*detailFade,0.0,1.0);
  }
  optical=mix(0.58,1.0,convection);
 }else if(layer==1){
  q=vec3<f32>(p.x-upper.x,(p.y-profile.x)*0.35,p.z-upper.y)/4096.0;
  var warp:vec4<f32>=vec4<f32>(0.5);if(detailed){warp=cloudVolume(q*0.5+vec3<f32>(evolution*0.25,0.0,evolution*0.5),max(0.0,lod-1.0));}
  var n:vec4<f32>=cloudVolume(q*2.0+(warp.gba-vec3<f32>(0.5))*0.20,lod+1.0);
  var threshold:f32=mix(0.78,0.20,coverage);shape=smoothstep(threshold-0.07,threshold+0.16,n.g*0.62+n.a*0.38);
  var sheet:f32=smoothstep(0.65,0.97,coverage);shape=mix(shape,mix(0.55,0.95,n.r),sheet*0.80);
  vertical=smoothstep(0.0,0.13,height)*(1.0-smoothstep(0.55,1.0,height));optical=0.38;
 }else{
  var angle:f32=0.32175055;
  var along:f32=p.x*cos(angle)+p.z*sin(angle)-upper.z;
  var across:f32=-p.x*sin(angle)+p.z*cos(angle)-upper.w;
  q=vec3<f32>(along*0.25+(height-0.5)*1000.0,(p.y-profile.x)*2.0,across*2.0)/4096.0;
  var warp:vec4<f32>=vec4<f32>(0.5);if(detailed){warp=cloudVolume(q*0.5+vec3<f32>(evolution*0.25,evolution*0.5,0.0),lod);}
  var n:vec4<f32>=cloudVolume(q+(warp.gba-vec3<f32>(0.5))*0.28,lod+1.0);
  var filament:f32=0.5+0.5*sin(q.z*50.265482+warp.a*7.0+n.a*3.0)*(1.0-smoothstep(60.0,260.0,footprint));
  var wisps:f32=n.g*0.52+n.a*0.30+filament*0.18;
  shape=smoothstep(mix(0.78,0.24,coverage),mix(0.92,0.72,coverage),wisps);
  shape=mix(shape,mix(0.30,0.80,n.a),smoothstep(0.78,1.0,coverage)*0.65);
  vertical=smoothstep(0.0,0.18,height)*(1.0-smoothstep(0.50,1.0,height));optical=0.12;
 }
 return max(0.0,shape*vertical*optical*cloudRow(11).w);
}
fn cloudDensity(p:vec3<f32>,detailed:bool,footprint:f32)->f32 {
 var density:f32=0.0;for(var layer:i32=0;layer<3;layer++){density+=cloudLayerDensity(p,layer,detailed,footprint);}return density;
}
fn cloudPhase(cosine:f32,g:f32)->f32 {return (1.0-g*g)/pow(max(1.0+g*g-2.0*g*cosine,0.03),1.5);}
fn cloudLight(p:vec3<f32>,rd:vec3<f32>,sun:vec3<f32>,density:f32)->vec3<f32> {
 var opticalDepth:f32=0.0;var distance:f32=75.0;var stride:f32=150.0;
 for(var probe:i32=0;probe<2;probe++){opticalDepth+=cloudDensity(p+sun*distance,false,200.0)*stride;distance+=stride;stride*=2.4;}
 var directTransmission:f32=exp(-opticalDepth*0.75);var multipleScatter:f32=exp(-opticalDepth*0.18)*0.22;
 var cosine:f32=clamp(dot(rd,sun),-1.0,1.0);
 var phase:f32=min(cloudPhase(cosine,0.64)*0.75+cloudPhase(cosine,-0.22)*0.25,7.0);
 var powder:f32=1.0-exp(-density*220.0);var silver:f32=mix(1.0,1.0+powder*0.60,smoothstep(0.2,0.95,cosine));
 var ambientTint:vec3<f32>=mix(vec3<f32>(0.48,0.58,0.72),max(cloudRow(11).rgb,vec3<f32>(0.02)),0.55);
 var ambient:vec3<f32>=ambientTint*cloudRow(10).w*(0.95+multipleScatter);
 var direct:vec3<f32>=cloudRow(10).rgb*(directTransmission*(0.32+phase*0.23)*silver+multipleScatter*0.60);
 return max(ambient+direct,vec3<f32>(0.0));
}
fn cloudSphere(ro:vec3<f32>,rd:vec3<f32>,radius:f32)->vec2<f32> {
 var b:f32=dot(ro,rd);var c:f32=dot(ro,ro)-radius*radius;var d:f32=b*b-c;
 if(d<0.0){return vec2<f32>(1.0,-1.0);}var root:f32=sqrt(d);return vec2<f32>(-b-root,-b+root);
}
fn cloudInterval(ro:vec3<f32>,rd:vec3<f32>,layer:i32)->vec2<f32> {
 var profile:vec4<f32>=cloudRow(6+layer);var radius:f32=cloudRow(3).w;
 var outer:vec2<f32>=cloudSphere(ro,rd,6371000.0+profile.x+profile.y);
 var lo:f32=max(0.0,outer.x);var hi:f32=min(radius,outer.y);
 if(hi<=lo){return vec2<f32>(1.0,-1.0);}
 var inner:vec2<f32>=cloudSphere(ro,rd,6371000.0+profile.x);
 if(inner.y>inner.x){if(lo<inner.x&&inner.x>0.0){hi=min(hi,inner.x);}else if(lo<inner.y){lo=max(lo,inner.y);}}
 var ground:vec2<f32>=cloudSphere(ro,rd,6371000.0);
 if(ground.y>ground.x&&ground.x>=0.0){hi=min(hi,ground.x);}
 return vec2<f32>(lo,hi);
}
fn cloudComposite(localRay:vec3<f32>,background:vec4<f32>)->vec4<f32> {
 if(cloudRow(0).w<0.5){return background;}
 var ro:vec3<f32>=cloudRow(0).xyz;
 var rd:vec3<f32>=normalize(cloudRow(1).xyz*localRay.x+cloudRow(2).xyz*localRay.y+cloudRow(3).xyz*localRay.z);
 var sun:vec3<f32>=normalize(cloudRow(9).xyz);
 var radiance:vec3<f32>=vec3<f32>(0.0);var transmittance:f32=1.0;var used:i32=0;
 for(var layerPass:i32=0;layerPass<3;layerPass++){
  var chosen:i32=-1;var nearest:f32=100000000.0;
  for(var layer:i32=0;layer<3;layer++){
   if((used&(1<<u32(layer)))!=0){continue;}
   var interval:vec2<f32>=cloudInterval(ro,rd,layer);
   if(interval.y>interval.x&&interval.x<nearest){chosen=layer;nearest=interval.x;}
  }
  if(chosen<0){break;}used|=1<<u32(chosen);
  var interval:vec2<f32>=cloudInterval(ro,rd,chosen);var stride:f32=(interval.y-interval.x)/24.0;
  // Stable sub-step sampling avoids a frame-random grain over the authored sky.
  for(var step:i32=0;step<24;step++){
   if(transmittance<0.015){break;}
   var t:f32=interval.x+(f32(step)+0.5)*stride;var p:vec3<f32>=ro+rd*t;
   var fade:f32=1.0-smoothstep(cloudRow(3).w*0.80,cloudRow(3).w,t);
   var density:f32=cloudLayerDensity(p,chosen,true,stride)*fade;
   if(density>0.000015){var alpha:f32=1.0-exp(-density*stride*0.75);radiance+=cloudLight(p,rd,sun,density)*(transmittance*alpha);transmittance*=1.0-alpha;}
  }
 }
 return vec4<f32>(radiance+background.rgb*transmittance,1.0);
}
// END GENERATED CLOUDS
fn theatreTexel(tile:u32, p:vec2<i32>)->vec4<f32>{let xy=vec2<u32>(p & vec2<i32>(127)); let n=tile*16384u+xy.y*128u+xy.x; return unpack4x8unorm(playerTexture.pixels[arrayLength(&playerTexture.pixels)-81920u+n]);}
fn theatreSample(d:vec4<f32>)->vec4<f32>{
 let p=fract(d.xy)*128.0-0.5; let i=vec2<i32>(floor(p)); let f=fract(p);
 let tile=select(u32(round(d.w))-100u,4u,d.w==114.);
 let c=mix(mix(theatreTexel(tile,i), theatreTexel(tile,i+vec2<i32>(1,0)),f.x),
           mix(theatreTexel(tile,i+vec2<i32>(0,1)),theatreTexel(tile,i+vec2<i32>(1,1)),f.x),f.y);
 return vec4<f32>(mix(vec3<f32>(0.22,0.025,0.04),c.rgb,c.a)*d.z*vec3<f32>(1.08,1.0,0.9),1.0);
}
fn tableRead(n:u32)->vec4<f32>{return unpack4x8unorm(playerTexture.pixels[arrayLength(&playerTexture.pixels)-81920u-4543844u+n]);}
fn tableEyeValue()->vec3<f32>{let b=arrayLength(&playerTexture.pixels)-81920u-4543844u;return vec3<f32>(bitcast<f32>(playerTexture.pixels[b]),bitcast<f32>(playerTexture.pixels[b+1u]),bitcast<f32>(playerTexture.pixels[b+2u]));}
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

// BEGIN GENERATED MEADOW GRASS
fn grassTexel(map:i32,level:i32,p:vec2<i32>)->vec4<f32>{
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
// END GENERATED MEADOW GRASS
// BEGIN GENERATED CAVE ROCK
fn rockTexel(kind:i32,map:i32,level:i32,p:vec2<i32>)->vec4<f32>{
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
// END GENERATED CAVE ROCK
// BEGIN GENERATED FINE LOAM
fn dirtTexel(map:i32,level:i32,p:vec2<i32>)->vec4<f32>{
 let size=512>>u32(level);let xy=vec2<u32>(p & vec2<i32>(size-1));let offset=u32((262144-size*size)*4/3);
 return tableRead(1747632u+u32(map)*349525u+offset+xy.y*u32(size)+xy.x);
}
fn dirtMap(map:i32,uv:vec2<f32>,lod:f32)->vec4<f32>{
 let level=i32(clamp(floor(lod),0.,9.));let size=512>>u32(level);
 let p=fract(uv)*f32(size)-.5;let i=vec2<i32>(floor(p));let f=fract(p);
 return mix(mix(dirtTexel(map,level,i),dirtTexel(map,level,i+vec2<i32>(1,0)),f.x),mix(dirtTexel(map,level,i+vec2<i32>(0,1)),dirtTexel(map,level,i+vec2<i32>(1,1)),f.x),f.y);
}
fn dirtDiffuse(rgb:vec3<f32>,metres:vec2<f32>,footprint:f32)->vec3<f32>{
 let lod=clamp(log2(max(footprint*1024.,1.)),0.,9.);
 let a=dirtMap(0,metres*2.,lod);let nh=dirtMap(1,metres*2.,lod);
 let up=cloudRow(2).xyz;var axis=0;
 if(abs(up.y)>abs(up.x)){axis=1;}if(abs(up.z)>abs(up[axis])){axis=2;}
 var N=vec3<f32>(0);N[axis]=select(1.,-1.,up[axis]<0.);
 var T=vec3<f32>(0);T[(axis+1)%3]=1.;var B=vec3<f32>(0);B[(axis+2)%3]=1.;
 let nm=normalize(nh.xyz*2.-1.);let n=normalize(T*nm.x+B*nm.y+N*nm.z);
 var L=cloudRow(9).xyz;L=L/max(length(L),.0001);
 let relief=clamp((.45+.55*max(dot(n,L),0.))/(.45+.55*max(dot(N,L),0.)),.65,1.35);
 let detail=1.-smoothstep(5.,9.,lod);
 let albedo=clamp(a.rgb/vec3<f32>(0.29019608,0.20784314,0.12156863),vec3<f32>(.45),vec3<f32>(1.65));
 let cavity=mix(.90,1.,nh.a)*mix(.9,1.,a.a);
 return rgb*mix(vec3<f32>(1),albedo*relief*cavity,detail);
}
// END GENERATED FINE LOAM
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
// BEGIN GENERATED EXPANSE SKY
// Distant, fictional blue-white sky. Direction-space noise has no UV seam or
// camera translation. No cloud/weather data or scene lighting is changed here.
fn celestialHash(p:vec3<f32>)->f32 {
 return fract(sin(dot(p,vec3<f32>(127.1,311.7,74.7)))*43758.5453);
}
fn celestialNoise(p:vec3<f32>)->f32 {
 var cell:vec3<f32>=floor(p);
 var f:vec3<f32>=fract(p);
 f=f*f*(vec3<f32>(3.0)-2.0*f);
 return mix(mix(mix(celestialHash(cell),celestialHash(cell+vec3<f32>(1.0,0.0,0.0)),f.x),mix(celestialHash(cell+vec3<f32>(0.0,1.0,0.0)),celestialHash(cell+vec3<f32>(1.0,1.0,0.0)),f.x),f.y),mix(mix(celestialHash(cell+vec3<f32>(0.0,0.0,1.0)),celestialHash(cell+vec3<f32>(1.0,0.0,1.0)),f.x),mix(celestialHash(cell+vec3<f32>(0.0,1.0,1.0)),celestialHash(cell+vec3<f32>(1.0,1.0,1.0)),f.x),f.y),f.z);
}
fn celestialFilaments(p:vec3<f32>)->f32 {
 return celestialNoise(p)*0.55+celestialNoise(p*2.03+vec3<f32>(13.1))*0.28+celestialNoise(p*4.11+vec3<f32>(7.7))*0.12+celestialNoise(p*8.21)*0.05;
}
fn celestialStars(ray:vec3<f32>,scale:f32,occupancy:f32,gain:f32)->vec3<f32> {
 // Centers stay inside their cells, so neighboring cells cannot clip a star.
 var p:vec3<f32>=ray*scale;
 var cell:vec3<f32>=floor(p);
 var seed:f32=celestialHash(cell);
 var center:vec3<f32>=vec3<f32>(celestialHash(cell+vec3<f32>(17.0)),celestialHash(cell+vec3<f32>(43.0)),celestialHash(cell+vec3<f32>(91.0)))*0.5+vec3<f32>(0.25);
 var distance:f32=length(fract(p)-center);
 var radius:f32=mix(0.055,0.14,pow(seed,12.0));
 // Preserve point energy as a star becomes smaller than a render pixel.
 var footprint:f32=length(fwidth(p))*0.5;
 var widthSquared:f32=radius*radius+footprint*footprint;
 var core:f32=exp(-distance*distance/widthSquared)*radius*radius/widthSquared;
 var halo:f32=exp(-distance*distance/(radius*radius*5.0))*0.07;
 var present:f32=step(1.0-occupancy,celestialHash(cell+vec3<f32>(113.0)));
 var tint:vec3<f32>=mix(vec3<f32>(0.55,0.72,1.0),vec3<f32>(0.94,0.97,1.0),seed);
 return tint*(core+halo)*present*gain;
}
fn celestialSky(ray:vec3<f32>,hour:f32)->vec3<f32> {
 // One revolution per day; wrapping is continuous at midnight. This is an
 // authored celestial sphere, not an Earth catalog or a foreground galaxy.
 var angle:f32=hour*0.2617993878;
 var pole:vec3<f32>=normalize(vec3<f32>(0.18,0.83,0.53));
 var sky:vec3<f32>=ray*cos(angle)+cross(pole,ray)*sin(angle)+pole*dot(pole,ray)*(1.0-cos(angle));
 var latitude:f32=dot(sky,normalize(vec3<f32>(0.34,0.58,-0.74)));
 var broad:f32=exp(-latitude*latitude*18.0);
 var ribbon:f32=exp(-latitude*latitude*95.0);
 var filaments:f32=celestialFilaments(sky*8.0+vec3<f32>(31.0));
 var dust:f32=smoothstep(0.38,0.64,celestialFilaments(sky*15.0+vec3<f32>(71.0)));
 var structure:f32=smoothstep(0.24,0.76,filaments);
 var glow:vec3<f32>=vec3<f32>(0.055,0.095,0.18)*broad*structure;
 glow+=vec3<f32>(0.11,0.16,0.23)*ribbon*structure*(1.0-dust*0.88);
 var wisp:f32=pow(max(0.0,celestialFilaments(sky*5.0+vec3<f32>(103.0))-0.38),2.0);
 glow+=vec3<f32>(0.10,0.23,0.38)*wisp*broad;
 var stars:vec3<f32>=celestialStars(sky,150.0,0.30,0.68);
 stars+=celestialStars(sky+vec3<f32>(7.0),310.0,0.38,0.30);
 stars+=celestialStars(sky+vec3<f32>(19.0),95.0,0.07,1.65);
 stars+=celestialStars(sky+vec3<f32>(29.0),420.0,0.52,0.20)*broad;
 return glow+stars*(1.0-dust*ribbon*0.65);
}
fn skyProfile0(z:f32)->vec3<f32>{
 if(z<=0.5){return mix(vec3<f32>(0.007843137255,0.02745098039,0.09803921569),vec3<f32>(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.02745098039,0.06666666667,0.1529411765),vec3<f32>(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3<f32>(0.06274509804,0.1254901961,0.2470588235);
}
fn skyProfile1(z:f32)->vec3<f32>{
 if(z<=0.5){return mix(vec3<f32>(0.007843137255,0.02745098039,0.09803921569),vec3<f32>(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.02745098039,0.06666666667,0.1529411765),vec3<f32>(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3<f32>(0.06274509804,0.1254901961,0.2470588235);
}
fn skyProfile2(z:f32)->vec3<f32>{
 if(z<=0.5){return mix(vec3<f32>(0.01960784314,0.05098039216,0.1647058824),vec3<f32>(0.06274509804,0.137254902,0.2901960784),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.06274509804,0.137254902,0.2901960784),vec3<f32>(0.1450980392,0.2705882353,0.431372549),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3<f32>(0.1450980392,0.2705882353,0.431372549);
}
fn skyProfile3(z:f32)->vec3<f32>{
 if(z<=0.2){return mix(vec3<f32>(0.003921568627,0.03921568627,0.2117647059),vec3<f32>(0.05098039216,0.1294117647,0.3607843137),clamp((z-0.0)/0.2,0.0,1.0));}
 if(z<=0.45){return mix(vec3<f32>(0.05098039216,0.1294117647,0.3607843137),vec3<f32>(0.1568627451,0.2588235294,0.5803921569),clamp((z-0.2)/0.25,0.0,1.0));}
 if(z<=0.65){return mix(vec3<f32>(0.1568627451,0.2588235294,0.5803921569),vec3<f32>(0.2392156863,0.3725490196,0.7254901961),clamp((z-0.45)/0.2,0.0,1.0));}
 if(z<=0.85){return mix(vec3<f32>(0.2392156863,0.3725490196,0.7254901961),vec3<f32>(0.3607843137,0.5176470588,0.8196078431),clamp((z-0.65)/0.2,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.3607843137,0.5176470588,0.8196078431),vec3<f32>(0.4823529412,0.6549019608,0.8862745098),clamp((z-0.85)/0.15,0.0,1.0));}
 return vec3<f32>(0.4823529412,0.6549019608,0.8862745098);
}
fn skyProfile4(z:f32)->vec3<f32>{
 if(z<=0.45){return mix(vec3<f32>(0.02352941176,0.1529411765,0.3607843137),vec3<f32>(0.0431372549,0.2,0.4),clamp((z-0.0)/0.45,0.0,1.0));}
 if(z<=0.75){return mix(vec3<f32>(0.0431372549,0.2,0.4),vec3<f32>(0.09019607843,0.2470588235,0.4470588235),clamp((z-0.45)/0.3,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.09019607843,0.2470588235,0.4470588235),vec3<f32>(0.1764705882,0.3294117647,0.5529411765),clamp((z-0.75)/0.25,0.0,1.0));}
 return vec3<f32>(0.1764705882,0.3294117647,0.5529411765);
}
fn skyProfile5(z:f32)->vec3<f32>{
 if(z<=0.35){return mix(vec3<f32>(0.3490196078,0.4549019608,0.6392156863),vec3<f32>(0.3333333333,0.4392156863,0.6235294118),clamp((z-0.0)/0.35,0.0,1.0));}
 if(z<=0.7){return mix(vec3<f32>(0.3333333333,0.4392156863,0.6235294118),vec3<f32>(0.2980392157,0.4156862745,0.6117647059),clamp((z-0.35)/0.35,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.2980392157,0.4156862745,0.6117647059),vec3<f32>(0.3294117647,0.4745098039,0.6901960784),clamp((z-0.7)/0.3,0.0,1.0));}
 return vec3<f32>(0.3294117647,0.4745098039,0.6901960784);
}
fn skyProfile6(z:f32)->vec3<f32>{
 if(z<=0.35){return mix(vec3<f32>(0.3490196078,0.4549019608,0.6392156863),vec3<f32>(0.3333333333,0.4392156863,0.6235294118),clamp((z-0.0)/0.35,0.0,1.0));}
 if(z<=0.7){return mix(vec3<f32>(0.3333333333,0.4392156863,0.6235294118),vec3<f32>(0.2980392157,0.4156862745,0.6117647059),clamp((z-0.35)/0.35,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.2980392157,0.4156862745,0.6117647059),vec3<f32>(0.3294117647,0.4745098039,0.6901960784),clamp((z-0.7)/0.3,0.0,1.0));}
 return vec3<f32>(0.3294117647,0.4745098039,0.6901960784);
}
fn skyProfile7(z:f32)->vec3<f32>{
 if(z<=0.45){return mix(vec3<f32>(0.3882352941,0.5137254902,0.6705882353),vec3<f32>(0.3960784314,0.5215686275,0.6745098039),clamp((z-0.0)/0.45,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.3960784314,0.5215686275,0.6745098039),vec3<f32>(0.5294117647,0.6274509804,0.7490196078),clamp((z-0.45)/0.55,0.0,1.0));}
 return vec3<f32>(0.5294117647,0.6274509804,0.7490196078);
}
fn skyProfile8(z:f32)->vec3<f32>{
 if(z<=0.45){return mix(vec3<f32>(0.02352941176,0.1529411765,0.3607843137),vec3<f32>(0.0431372549,0.2,0.4),clamp((z-0.0)/0.45,0.0,1.0));}
 if(z<=0.75){return mix(vec3<f32>(0.0431372549,0.2,0.4),vec3<f32>(0.09019607843,0.2470588235,0.4470588235),clamp((z-0.45)/0.3,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.09019607843,0.2470588235,0.4470588235),vec3<f32>(0.1764705882,0.3294117647,0.5529411765),clamp((z-0.75)/0.25,0.0,1.0));}
 return vec3<f32>(0.1764705882,0.3294117647,0.5529411765);
}
fn skyProfile9(z:f32)->vec3<f32>{
 if(z<=0.2){return mix(vec3<f32>(0.003921568627,0.03921568627,0.2117647059),vec3<f32>(0.05098039216,0.1294117647,0.3607843137),clamp((z-0.0)/0.2,0.0,1.0));}
 if(z<=0.45){return mix(vec3<f32>(0.05098039216,0.1294117647,0.3607843137),vec3<f32>(0.1568627451,0.2588235294,0.5803921569),clamp((z-0.2)/0.25,0.0,1.0));}
 if(z<=0.65){return mix(vec3<f32>(0.1568627451,0.2588235294,0.5803921569),vec3<f32>(0.2392156863,0.3725490196,0.7254901961),clamp((z-0.45)/0.2,0.0,1.0));}
 if(z<=0.85){return mix(vec3<f32>(0.2392156863,0.3725490196,0.7254901961),vec3<f32>(0.3607843137,0.5176470588,0.8196078431),clamp((z-0.65)/0.2,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.3607843137,0.5176470588,0.8196078431),vec3<f32>(0.4823529412,0.6549019608,0.8862745098),clamp((z-0.85)/0.15,0.0,1.0));}
 return vec3<f32>(0.4823529412,0.6549019608,0.8862745098);
}
fn skyProfile10(z:f32)->vec3<f32>{
 if(z<=0.5){return mix(vec3<f32>(0.007843137255,0.03137254902,0.1176470588),vec3<f32>(0.03921568627,0.09411764706,0.2235294118),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.03921568627,0.09411764706,0.2235294118),vec3<f32>(0.09411764706,0.1921568627,0.3529411765),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3<f32>(0.09411764706,0.1921568627,0.3529411765);
}
fn skyProfile11(z:f32)->vec3<f32>{
 if(z<=0.5){return mix(vec3<f32>(0.007843137255,0.02745098039,0.09803921569),vec3<f32>(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.02745098039,0.06666666667,0.1529411765),vec3<f32>(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3<f32>(0.06274509804,0.1254901961,0.2470588235);
}
fn skyProfile12(z:f32)->vec3<f32>{
 if(z<=0.5){return mix(vec3<f32>(0.007843137255,0.02745098039,0.09803921569),vec3<f32>(0.02745098039,0.06666666667,0.1529411765),clamp((z-0.0)/0.5,0.0,1.0));}
 if(z<=1.0){return mix(vec3<f32>(0.02745098039,0.06666666667,0.1529411765),vec3<f32>(0.06274509804,0.1254901961,0.2470588235),clamp((z-0.5)/0.5,0.0,1.0));}
 return vec3<f32>(0.06274509804,0.1254901961,0.2470588235);
}
fn skyPalette(minute:f32,z:f32)->vec3<f32>{
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
fn expanseSky(direction:vec3<f32>,kind:f32)->vec4<f32>{
 let ray=normalize(direction);
 let hour=fract(-kind-10.0)*24.0;
 let angle=(hour-6.0)/12.0*3.14159265359;
 let sun=vec3<f32>(0.0,sin(angle),-cos(angle));
 let elevation=asin(clamp(ray.y,0.0,1.0))*57.295779513;
 var color=skyPalette(hour*60.0,1.0-elevation/90.0);
 let sunAltitude=(18.0-hour)*15.0;
 let azimuthCos=clamp(dot(sun.xz,ray.xz)/max(length(sun.xz)*length(ray.xz),0.00001),-1.0,1.0);
 let toward=pow((1.0+azimuthCos)*0.5,0.75);
 let opposite=pow((1.0-azimuthCos)*0.5,1.25);
 let strength=smoothstep(15.0,17.5,hour)*(1.0-smoothstep(18.0,19.3,hour));
 let shadowHeight=max(0.0,-sunAltitude)*1.65;
 let shadow=smoothstep(0.0,5.0,-sunAltitude)*opposite*(1.0-smoothstep(shadowHeight,shadowHeight+12.0,elevation))*(1.0-smoothstep(18.75,19.5,hour));
 color=mix(color,vec3<f32>(0.025,0.062,0.16),shadow*0.72);
 let bandHeight=12.0+max(0.0,sunAltitude)*0.55;
 let warm=strength*toward*exp(-pow(elevation/bandHeight,1.45));
 color=mix(color,vec3<f32>(0.72,0.8,0.86),smoothstep(0.0,0.45,warm)*0.82);
 color=mix(color,vec3<f32>(0.95,0.95,0.8),smoothstep(0.18,0.66,warm));
 color=mix(color,vec3<f32>(1.0,0.9,0.32),smoothstep(0.48,1.0,warm));
 // Distant celestial light is behind the original sun/moon and cloud pass.
 let night=1.0-smoothstep(-0.38,-0.08,sun.y);
 color+=celestialSky(ray,hour)*night*smoothstep(-0.02,0.22,ray.y);
 // Angular disks are an adaptation of the original billboard celestials.
 let sunDisk=smoothstep(cos(0.0048),cos(0.0043),dot(ray,sun));
 let moonDisk=smoothstep(cos(0.0048),cos(0.0043),dot(ray,-sun));
 color=mix(color,vec3<f32>(1.0,1.0,0.8),sunDisk);
 color=mix(color,vec3<f32>(205.0,215.0,233.0)/255.0,moonDisk);
 return vec4<f32>(color,1.0);
}
// END GENERATED EXPANSE SKY
@fragment fn fs_main(i:VertexOutput)->@location(0) vec4<f32>{
 let footprint=max(length(dpdx(i.data.xy)),length(dpdy(i.data.xy)));
 let aspect=abs(dpdy(i.screen.y))/max(abs(dpdx(i.screen.x)),0.000001);

 // Derivatives are evaluated uniformly before the material branch.
 let cell=i.data.xy*24.0;let width=max(fwidth(cell),vec2<f32>(0.00001));
 let rockFootprint=max(length(dpdx(i.soilUV)),length(dpdy(i.soilUV)));
 let soilFade=1.0-smoothstep(0.008,0.035,max(length(dpdx(i.soilUV)),length(dpdy(i.soilUV))));
 // Incense smoke: soft radial coverage, blended back-to-front with scene depth.
 if i.material==121. {
  let radius=dot(i.data.xy,i.data.xy);
  let coverage=i.data.z*pow(max(0.,1.-radius),2.);
  if coverage<0.001 {discard;}
  return vec4<f32>(0.63,0.66,0.67,coverage);
 }
 if i.material<=-4096.{return vec4<f32>(rockDiffuse(i.data.rgb,i.soilUV,i32(-i.material-4096.),rockFootprint),1.);}
 if ((i.material>=112. && i.material<=116. && i.material!=114.)){return exteriorSurface(i.data,max(length(dpdx(i.data.xyz)),length(dpdy(i.data.xyz))));}
 if i.material>=117. && i.material<=120.{return carpetSurface(i.data,max(length(dpdx(i.data.xyz)),length(dpdy(i.data.xyz))));}
 if i.material>=106.0 && i.material<=111.0{return tableSurface(i.data,footprint);}
 if i.material==105.0 {
  let xy=vec2<u32>(clamp(i.data.xy*vec2<f32>(512.,288.),vec2<f32>(0.),vec2<f32>(511.,287.)));
  let n=arrayLength(&playerTexture.pixels)-81920u-4543844u-147456u+xy.y*512u+xy.x;
  return vec4<f32>(unpack4x8unorm(playerTexture.pixels[n]).rgb,1.);
 }
 if i.material>=100.0 && (i.material<=104.0 || i.material==114.0){return theatreSample(i.data);}
 if i.material>=4096.0{return vec4<f32>(mix(i.data.rgb,soilDiffuse(i.data.rgb,i.soilUV,soilFade,rockFootprint),1.0-smoothstep(192.0,320.0,1.0/i.position.w)),1.0);}
 if i.material<=-9.5{return cloudComposite(i.data.xyz,expanseSky(i.data.xyz,i.material));}
 if i.material<=-3.5{let e=colorEmotion(i.data,i.material,i.screen,aspect);if e.a<=0.001{discard;}return e;}
 if i.material < -1.5 && i.material > -2.5 {let t=playerSample(i.data.xy);if t.a<=0.002{discard;}return vec4<f32>(t.rgb*i.data.z,t.a);}
 if i.material < -0.5 {let line=clamp((vec2<f32>(0.03)-fract(cell))/width+vec2<f32>(0.5),vec2<f32>(0.0),vec2<f32>(1.0));let coverage=max(line.x,line.y)*(1.0-smoothstep(0.35,1.0,max(width.x,width.y)));let base=select(vec3<f32>(0.27,0.40,0.22),vec3<f32>(0.45,0.29,0.13),i.material < -2.5);return vec4<f32>(base*i.data.z*(1.0-coverage),1.0);}
 return vec4<f32>(i.data.rgb,1.0);
}
