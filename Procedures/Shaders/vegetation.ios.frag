#version 300 es
precision highp float;
precision highp int;
in vec3 worldPosition;in vec3 surfaceNormal;in vec2 leafUV;in vec3 baseColor;
flat in vec4 material;flat in float barkRidges;in vec4 shadowPosition;
uniform vec3 sunDirection;uniform vec3 sunColor;uniform vec3 ambientColor;
uniform sampler2D shadowDepth;uniform bool shadowsEnabled;
out vec4 pixel;


// BEGIN TREE IMPOSTORS: baked from actual detailed branch/foliage meshes.
uniform sampler2D treeAtlas;
vec4 treeFetch(ivec2 p){
 p=clamp(p,ivec2(0),ivec2(3071,3583));
 return texelFetch(treeAtlas,p,0);
}
vec4 treeLayer(vec2 uv,float tile,int layer){
 int index=clamp(int(tile+0.5),0,111);ivec2 start=ivec2((index%16)*192,(layer*7+index/16)*256);
 vec2 p=clamp(uv,vec2(0.0),vec2(1.0))*vec2(192.0,256.0)-0.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 ivec2 a=clamp(i,ivec2(0),ivec2(191,255));ivec2 b=clamp(i+ivec2(1),ivec2(0),ivec2(191,255));
 return mix(mix(treeFetch(start+a),treeFetch(start+ivec2(b.x,a.y)),f.x),mix(treeFetch(start+ivec2(a.x,b.y)),treeFetch(start+b),f.x),f.y);
}
vec4 treeSurface(vec2 uv,float tile,float cover){
 vec4 leaf=treeLayer(uv,tile,1);vec2 cell=floor(uv*vec2(48.0,64.0));float seed=fract(sin(dot(cell,vec2(127.1,311.7)))*43758.5453);
 if(leaf.a>0.35&&seed<cover)return vec4(leaf.rgb/max(leaf.a,0.001),0.5);
 vec4 wood=treeLayer(uv,tile,0);if(wood.a>0.35)return vec4(wood.rgb/max(wood.a,0.001),1.0);
 return vec4(0.0);
}
vec3 treeWood(float tile){int species=clamp(int(tile)/16,0,6);if(species==0)return vec3(0.36,0.32,0.27);if(species==1)return vec3(0.34,0.32,0.30);if(species==2)return vec3(0.30,0.25,0.21);if(species==4)return vec3(0.34,0.31,0.27);if(species==5)return vec3(0.22,0.18,0.16);if(species==6)return vec3(0.48,0.39,0.24);return vec3(0.30,0.23,0.19);}
// END TREE IMPOSTORS
float paFoliage(vec2 uv,float kind){
 if(kind>4.5){float halfWidth=(1.0-uv.y)*0.48;return min(halfWidth-abs(uv.x-0.5),min(uv.y,1.0-uv.y));}
 vec2 p=uv-0.5;
 float envelope=1.0-dot(p/vec2(0.50,0.47),p/vec2(0.50,0.47));
 vec2 cell=floor(uv*vec2(48.0,40.0));
 float seed=fract(sin(dot(cell,vec2(127.1,311.7)))*43758.5453);
 vec2 q=fract(uv*vec2(48.0,40.0))-0.5;
 q-=vec2(seed-0.5,fract(seed*17.0)-0.5)*0.5;
 float needle=-1.0;
 for(int i=0;i<5;i++){
  float angle=seed*6.2831853+float(i)*0.52;
  vec2 v=vec2(q.x*cos(angle)-q.y*sin(angle),q.x*sin(angle)+q.y*cos(angle));
  vec2 scaled=v/vec2(0.47,kind<3.5?0.045:0.085);
  needle=max(needle,1.0-dot(scaled,scaled));
 }
 return min(envelope,needle);
}
float ellipse(vec2 p,vec2 c,vec2 r){vec2 q=(p-c)/r;return 1.0-dot(q,q);}
float silhouette(vec2 uv,float kind){
 if(kind>2.5)return paFoliage(uv,kind);
 if(kind<1.5)return ellipse(uv,vec2(0.5),vec2(0.50,0.48));
vec2 cell=floor(uv*vec2(20.0,16.0));
 vec2 q=fract(uv*vec2(20.0,16.0))-0.5;
 float seed=fract(sin(dot(cell,vec2(127.1,311.7)))*43758.5453);
 q-=vec2(seed-0.5,fract(seed*17.0)-0.5)*0.65;
 float tilt=(seed-0.5)*1.4;
 q=mat2(cos(tilt),sin(tilt),-sin(tilt),cos(tilt))*q;
 float fine=1.0-dot(q/vec2(0.48,0.42),q/vec2(0.48,0.42));
 return min(fine,ellipse(uv,vec2(0.5),vec2(0.50,0.49))+(seed-0.5)*0.45);
}
float sunVisibility(vec3 n,vec3 light){
 if(!shadowsEnabled)return 1.0;
 vec3 q=shadowPosition.xyz/shadowPosition.w;q.xy=q.xy*0.5+0.5;
 if(any(lessThan(q,vec3(0.0)))||any(greaterThan(q,vec3(1.0))))return 1.0;
 vec2 texel=1.0/vec2(textureSize(shadowDepth,0));
 float bias=max(0.00015,0.0006*(1.0-abs(dot(n,light))));float sum=0.0;
 for(int x=-1;x<=1;x++)for(int y=-1;y<=1;y++)sum+=q.z-bias<=texture(shadowDepth,q.xy+vec2(x,y)*texel).r?1.0:0.0;
 return sum/9.0;
}
// BEGIN GENERATED WOODLAND MATERIALS
uniform sampler2D woodlandAtlas;
vec4 woodlandFetch(int tile,ivec2 p){p=ivec2((p.x%24+24)%24,(p.y%24+24)%24);return texelFetch(woodlandAtlas,ivec2(p.x,p.y+tile*24),0);}
vec3 woodlandAverage(int tile){
 if(tile==0)return vec3(0.39563589,0.60928649,0.30095316);
 if(tile==1)return vec3(0.19842048,0.26574755,0.18027642);
 if(tile==2)return vec3(0.19780773,0.26401825,0.17907816);
 if(tile==3)return vec3(0.33482435,0.23693491,0.14880174);
 if(tile==4)return vec3(0.09831836,0.15777505,0.26610839);
 if(tile==5)return vec3(0.28362609,0.20529684,0.12704248);
 if(tile==6)return vec3(0.74554058,0.68303377,0.49137391);
 if(tile==7)return vec3(0.66249319,0.57255583,0.43877996);
 if(tile==8)return vec3(0.46759940,0.42123502,0.37997685);
 if(tile==9)return vec3(0.49703159,0.43745915,0.38775191);
 if(tile==10)return vec3(0.32685185,0.27126225,0.22078568);
 if(tile==11)return vec3(0.39304194,0.34652097,0.27423747);
 if(tile==12)return vec3(0.38204657,0.30001362,0.25578704);
 if(tile==13)return vec3(0.19680011,0.25918437,0.17717865);
 if(tile==14)return vec3(0.19862473,0.26670071,0.18103894);
 if(tile==15)return vec3(0.19155093,0.14415850,0.23408224);
 if(tile==16)return vec3(0.11437908,0.33587282,0.56011029);
 if(tile==17)return vec3(0.26125408,0.28304058,0.31373230);
 if(tile==18)return vec3(0.25842184,0.28007217,0.31021923);
 if(tile==19)return vec3(0.29573121,0.32034995,0.35297522);
 if(tile==20)return vec3(0.26116558,0.28299973,0.31272467);
 if(tile==21)return vec3(0.25797249,0.28061683,0.31349401);
 if(tile==22)return vec3(0.26966231,0.29324619,0.32607571);
 if(tile==23)return vec3(0.26966912,0.29235430,0.32373366);
 if(tile==24)return vec3(0.02896242,0.01410675,0.03665577);
 if(tile==25)return vec3(0.03061683,0.06396378,0.07830882);
 if(tile==26)return vec3(0.01889978,0.03585920,0.04215686);
 if(tile==27)return vec3(0.03504902,0.01921977,0.04114243);
 if(tile==28)return vec3(0.09874047,0.15833333,0.11574755);
 if(tile==29)return vec3(0.08443627,0.10982435,0.15168845);
 if(tile==30)return vec3(0.07925517,0.09769199,0.14017565);
 if(tile==31)return vec3(0.09990468,0.10285267,0.15567130);
 if(tile==32)return vec3(0.07241966,0.07071078,0.11083197);
 if(tile==33)return vec3(0.16548203,0.31998230,0.17498638);
 if(tile==34)return vec3(0.16644880,0.32109205,0.17574891);
 if(tile==35)return vec3(0.16569308,0.31986656,0.17501362);
 if(tile==36)return vec3(0.16309913,0.31642157,0.17270561);
 if(tile==37)return vec3(0.96225490,0.52310730,0.14091095);
 if(tile==38)return vec3(0.97115332,0.56880447,0.17783905);
 if(tile==39)return vec3(0.98772467,0.65448666,0.23805147);
 if(tile==40)return vec3(0.94377723,0.45564406,0.10972903);
 if(tile==41)return vec3(0.97953431,0.61130174,0.20058551);
 if(tile==42)return vec3(0.97046569,0.56032135,0.16506672);
 if(tile==43)return vec3(0.98204657,0.62770289,0.22244009);
 if(tile==44)return vec3(0.97249455,0.57286901,0.17500681);
 if(tile==45)return vec3(0.95632489,0.50240332,0.13588644);
 if(tile==46)return vec3(0.03848039,0.04840686,0.08996460);
 if(tile==47)return vec3(0.33696215,0.34112881,0.35018382);
return vec3(0.3);}
vec3 woodlandSample(int tile,vec2 uv,float footprint){
 vec2 p=fract(uv)*24.0-0.5;ivec2 i=ivec2(floor(p));vec2 f=fract(p);
 vec3 color=mix(mix(woodlandFetch(tile,i).rgb,woodlandFetch(tile,i+ivec2(1,0)).rgb,f.x),mix(woodlandFetch(tile,i+ivec2(0,1)).rgb,woodlandFetch(tile,i+ivec2(1,1)).rgb,f.x),f.y);
 return mix(color,woodlandAverage(tile),smoothstep(1.0,8.0,footprint*24.0));
}
// END GENERATED WOODLAND MATERIALS
void main(){
 // Derivatives precede material branches on every backend.
 vec3 dpdx=dFdx(worldPosition),dpdy=dFdy(worldPosition);
 vec2 duvdx=dFdx(leafUV),duvdy=dFdy(leafUV);
 if(material.x>0.5&&material.x<5.5&&silhouette(leafUV,material.x)<0.0)discard;
 vec3 n=normalize(surfaceNormal);vec3 base=baseColor;
 if(material.x<0.5){
  float ridge=sin(leafUV.x*mod(barkRidges,1000.0)*6.2831853+sin(leafUV.y*5.0)*0.7);
  vec3 tangent=dpdx*duvdy.y-dpdy*duvdx.y;
  if(dot(tangent,tangent)>0.0000001)n=normalize(n+normalize(tangent)*ridge*0.28);
  base=woodlandSample(clamp(int(floor(barkRidges/1000.0)),8,12),leafUV,max(length(duvdx),length(duvdy)))*(0.86+0.14*ridge);
 }
 float transmission=material.y;
 if(material.x>0.5&&material.x<5.5&&!gl_FrontFacing)n=-n;
 if(material.x>5.5){
  vec4 surface=treeSurface(leafUV,barkRidges,material.y);if(surface.a<0.1)discard;
  float det=duvdx.x*duvdy.y-duvdx.y*duvdy.x;
  if(abs(det)>0.0000000001){vec3 right=normalize((dpdx*duvdy.y-dpdy*duvdx.y)/det);vec3 up=normalize((-dpdx*duvdy.x+dpdy*duvdx.x)/det);vec3 local=surface.rgb*2.0-1.0;n=normalize(right*local.x+up*local.y+n*local.z);}
  if(surface.a>0.75){base=treeWood(barkRidges);transmission=0.0;}else{transmission=0.35;}
 }
 vec3 light=normalize(sunDirection);vec3 view=normalize(-worldPosition);
 float direct=max(dot(n,light),0.0);float visibility=sunVisibility(n,light);
 float back=max(dot(-n,light),0.0);
 float forward=pow(max(dot(-view,light),0.0),3.0);
 float transmitted=transmission*back*(0.45+0.55*forward);
 float exponent=mix(96.0,3.0,clamp(material.z,0.0,1.0));
 vec3 halfVector=light+view;
 float specular=dot(halfVector,halfVector)>0.000001?pow(max(dot(n,normalize(halfVector)),0.0),exponent)*0.035:0.0;
 vec3 result=base*(ambientColor*material.w+sunColor*visibility*(direct+transmitted))+sunColor*specular*visibility;
 pixel=vec4(result,1.0);
}
