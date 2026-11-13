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
