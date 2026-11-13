#version 330 core
// Shadow depth pass uses the same material silhouette as the color pass.
in vec2 leafUV;flat in vec4 material;


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
flat in float barkRidges;
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
float spray(vec2 uv){vec2 cell=floor(uv*vec2(20.0,16.0));
 vec2 q=fract(uv*vec2(20.0,16.0))-0.5;
 float seed=fract(sin(dot(cell,vec2(127.1,311.7)))*43758.5453);
 q-=vec2(seed-0.5,fract(seed*17.0)-0.5)*0.65;
 float tilt=(seed-0.5)*1.4;
 q=mat2(cos(tilt),sin(tilt),-sin(tilt),cos(tilt))*q;
 float fine=1.0-dot(q/vec2(0.48,0.42),q/vec2(0.48,0.42));
 return min(fine,ellipse(uv,vec2(0.5),vec2(0.50,0.49))+(seed-0.5)*0.45);}
void main(){
 if(material.x>5.5){if(treeSurface(leafUV,barkRidges,material.y).a<0.1)discard;return;}
 if(material.x<0.5)return;
 float mask=ellipse(leafUV,vec2(0.5),vec2(0.50,0.48));
 if(material.x>1.5){mask=spray(leafUV);}
 if(material.x>2.5){mask=paFoliage(leafUV,material.x);}
 if(mask<0.0)discard;
}
