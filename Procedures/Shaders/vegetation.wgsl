// 16-float vertex ABI matches vegetation.gl.vert. Canonical projection depth 0..1.
struct Scene {viewProjection:mat4x4<f32>,sunViewProjection:mat4x4<f32>,sunDirection:vec4<f32>,sunColor:vec4<f32>,ambientColor:vec4<f32>,options:vec4<u32>};
@group(0) @binding(0) var<uniform> scene:Scene;
@group(0) @binding(1) var shadowDepth:texture_depth_2d;
struct Input {@location(0) position:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) color:vec3<f32>,@location(4) properties:vec4<f32>,@location(5) ridges:f32};
struct Output {@builtin(position) position:vec4<f32>,@location(0) worldPosition:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) color:vec3<f32>,@location(4) @interpolate(flat) properties:vec4<f32>,@location(5) @interpolate(flat) ridges:f32,@location(6) shadowPosition:vec4<f32>};
fn vegetationVertex(i:Input)->Output {var o:Output;o.position=scene.viewProjection*vec4<f32>(i.position,1.0);o.worldPosition=i.position;o.normal=i.normal;o.uv=i.uv;o.color=i.color;o.properties=i.properties;o.ridges=i.ridges;o.shadowPosition=scene.sunViewProjection*vec4<f32>(i.position,1.0);return o;}
@vertex fn vs_main(i:Input)->Output {return vegetationVertex(i);}
@vertex fn vs_shadow(i:Input)->Output {var o=vegetationVertex(i);o.position=o.shadowPosition;return o;}
// BEGIN TREE IMPOSTORS
struct TreeImpostors {pixels:array<u32>};
@group(0) @binding(3) var<storage,read> treeImpostors:TreeImpostors;
fn treeFetch(point:vec2<i32>)->vec4<f32>{let p=clamp(point,vec2<i32>(0),vec2<i32>(3071,3583));return unpack4x8unorm(treeImpostors.pixels[u32(p.y*3072+p.x)]);}
fn treeLayer(uv:vec2<f32>,tile:f32,layer:i32)->vec4<f32>{
 let index=clamp(i32(tile+0.5),0,111);let start=vec2<i32>((index%16)*192,(layer*7+index/16)*256);
 let p=clamp(uv,vec2<f32>(0.0),vec2<f32>(1.0))*vec2<f32>(192.0,256.0)-vec2<f32>(0.5);let i=vec2<i32>(floor(p));let f=fract(p);
 let a=clamp(i,vec2<i32>(0),vec2<i32>(191,255));let b=clamp(i+vec2<i32>(1),vec2<i32>(0),vec2<i32>(191,255));
 return mix(mix(treeFetch(start+a),treeFetch(start+vec2<i32>(b.x,a.y)),f.x),mix(treeFetch(start+vec2<i32>(a.x,b.y)),treeFetch(start+b),f.x),f.y);
}
fn treeSurface(uv:vec2<f32>,tile:f32,cover:f32)->vec4<f32>{
 let leaf=treeLayer(uv,tile,1);let cell=floor(uv*vec2<f32>(48.0,64.0));let seed=fract(sin(dot(cell,vec2<f32>(127.1,311.7)))*43758.5453);
 if leaf.a>0.35&&seed<cover{return vec4<f32>(leaf.rgb/max(leaf.a,0.001),0.5);}
 let wood=treeLayer(uv,tile,0);if wood.a>0.35{return vec4<f32>(wood.rgb/max(wood.a,0.001),1.0);}
 return vec4<f32>(0.0);
}
fn treeWood(tile:f32)->vec3<f32>{let species=clamp(i32(tile)/16,0,6);if species==0{return vec3<f32>(0.36,0.32,0.27);}if species==1{return vec3<f32>(0.34,0.32,0.30);}if species==2{return vec3<f32>(0.30,0.25,0.21);}if species==4{return vec3<f32>(0.34,0.31,0.27);}if species==5{return vec3<f32>(0.22,0.18,0.16);}if species==6{return vec3<f32>(0.48,0.39,0.24);}return vec3<f32>(0.30,0.23,0.19);}
// END TREE IMPOSTORS
fn paFoliage(uv:vec2<f32>,kind:f32)->f32 {
 if kind>4.5 {let halfWidth=(1.0-uv.y)*0.48;return min(halfWidth-abs(uv.x-0.5),min(uv.y,1.0-uv.y));}
 let p=uv-vec2<f32>(0.5);let rim=p/vec2<f32>(0.50,0.47);let envelope=1.0-dot(rim,rim);
 let cell=floor(uv*vec2<f32>(48.0,40.0));let seed=fract(sin(dot(cell,vec2<f32>(127.1,311.7)))*43758.5453);
 var q=fract(uv*vec2<f32>(48.0,40.0))-vec2<f32>(0.5);
 q-=vec2<f32>(seed-0.5,fract(seed*17.0)-0.5)*0.5;
 var needle=-1.0;
 for(var i=0;i<5;i++){
  let angle=seed*6.2831853+f32(i)*0.52;
  let v=vec2<f32>(q.x*cos(angle)-q.y*sin(angle),q.x*sin(angle)+q.y*cos(angle));
  let scaled=v/vec2<f32>(0.47,select(0.085,0.045,kind<3.5));
  needle=max(needle,1.0-dot(scaled,scaled));
 }
 return min(envelope,needle);
}
fn ellipse(p:vec2<f32>,c:vec2<f32>,r:vec2<f32>)->f32 {let q=(p-c)/r;return 1.0-dot(q,q);}
fn silhouette(uv:vec2<f32>,kind:f32)->f32 {
 if kind>2.5{return paFoliage(uv,kind);}
 if kind<1.5{return ellipse(uv,vec2<f32>(0.5),vec2<f32>(0.50,0.48));}
 let cell=floor(uv*vec2<f32>(20.0,16.0));
 var q=fract(uv*vec2<f32>(20.0,16.0))-vec2<f32>(0.5);
 let seed=fract(sin(dot(cell,vec2<f32>(127.1,311.7)))*43758.5453);
 q-=vec2<f32>(seed-0.5,fract(seed*17.0)-0.5)*0.65;
 let tilt=(seed-0.5)*1.4;
 q=mat2x2<f32>(vec2<f32>(cos(tilt),sin(tilt)),vec2<f32>(-sin(tilt),cos(tilt)))*q;
 let r=q/vec2<f32>(0.48,0.42);
 return min(1.0-dot(r,r),ellipse(uv,vec2<f32>(0.5),vec2<f32>(0.50,0.49))+(seed-0.5)*0.45);
}
fn visibility(shadowPosition:vec4<f32>,n:vec3<f32>,light:vec3<f32>)->f32 {
 if scene.options.x==0u{return 1.0;}
 var q=shadowPosition.xyz/shadowPosition.w;q=vec3<f32>(q.x*0.5+0.5,0.5-q.y*0.5,q.z);
 if any(q<vec3<f32>(0.0))||any(q>vec3<f32>(1.0)){return 1.0;}
 let size=vec2<i32>(textureDimensions(shadowDepth));let p=vec2<i32>(q.xy*vec2<f32>(size));
 let bias=max(0.00015,0.0006*(1.0-abs(dot(n,light))));var sum=0.0;
 for(var x=-1;x<=1;x++){for(var y=-1;y<=1;y++){let z=textureLoad(shadowDepth,clamp(p+vec2<i32>(x,y),vec2<i32>(0),size-vec2<i32>(1)),0);sum+=select(0.0,1.0,q.z-bias<=z);}}
 return sum/9.0;
}
// BEGIN GENERATED WOODLAND MATERIALS
struct WoodlandStorage {width:u32,height:u32,pixels:array<u32>};
@group(0) @binding(2) var<storage,read> woodlandStorage:WoodlandStorage;
fn woodlandFetch(tile:i32,point:vec2<i32>)->vec4<f32>{let p=(point%vec2<i32>(24)+vec2<i32>(24))%vec2<i32>(24);return unpack4x8unorm(woodlandStorage.pixels[4194306u+u32((p.y+tile*24)*24+p.x)]);}
fn woodlandAverage(tile:i32)->vec3<f32>{
 if tile==0{return vec3<f32>(0.39563589,0.60928649,0.30095316);}
 if tile==1{return vec3<f32>(0.19842048,0.26574755,0.18027642);}
 if tile==2{return vec3<f32>(0.19780773,0.26401825,0.17907816);}
 if tile==3{return vec3<f32>(0.33482435,0.23693491,0.14880174);}
 if tile==4{return vec3<f32>(0.09831836,0.15777505,0.26610839);}
 if tile==5{return vec3<f32>(0.28362609,0.20529684,0.12704248);}
 if tile==6{return vec3<f32>(0.74554058,0.68303377,0.49137391);}
 if tile==7{return vec3<f32>(0.66249319,0.57255583,0.43877996);}
 if tile==8{return vec3<f32>(0.46759940,0.42123502,0.37997685);}
 if tile==9{return vec3<f32>(0.49703159,0.43745915,0.38775191);}
 if tile==10{return vec3<f32>(0.32685185,0.27126225,0.22078568);}
 if tile==11{return vec3<f32>(0.39304194,0.34652097,0.27423747);}
 if tile==12{return vec3<f32>(0.38204657,0.30001362,0.25578704);}
 if tile==13{return vec3<f32>(0.19680011,0.25918437,0.17717865);}
 if tile==14{return vec3<f32>(0.19862473,0.26670071,0.18103894);}
 if tile==15{return vec3<f32>(0.19155093,0.14415850,0.23408224);}
 if tile==16{return vec3<f32>(0.11437908,0.33587282,0.56011029);}
 if tile==17{return vec3<f32>(0.26125408,0.28304058,0.31373230);}
 if tile==18{return vec3<f32>(0.25842184,0.28007217,0.31021923);}
 if tile==19{return vec3<f32>(0.29573121,0.32034995,0.35297522);}
 if tile==20{return vec3<f32>(0.26116558,0.28299973,0.31272467);}
 if tile==21{return vec3<f32>(0.25797249,0.28061683,0.31349401);}
 if tile==22{return vec3<f32>(0.26966231,0.29324619,0.32607571);}
 if tile==23{return vec3<f32>(0.26966912,0.29235430,0.32373366);}
 if tile==24{return vec3<f32>(0.02896242,0.01410675,0.03665577);}
 if tile==25{return vec3<f32>(0.03061683,0.06396378,0.07830882);}
 if tile==26{return vec3<f32>(0.01889978,0.03585920,0.04215686);}
 if tile==27{return vec3<f32>(0.03504902,0.01921977,0.04114243);}
 if tile==28{return vec3<f32>(0.09874047,0.15833333,0.11574755);}
 if tile==29{return vec3<f32>(0.08443627,0.10982435,0.15168845);}
 if tile==30{return vec3<f32>(0.07925517,0.09769199,0.14017565);}
 if tile==31{return vec3<f32>(0.09990468,0.10285267,0.15567130);}
 if tile==32{return vec3<f32>(0.07241966,0.07071078,0.11083197);}
 if tile==33{return vec3<f32>(0.16548203,0.31998230,0.17498638);}
 if tile==34{return vec3<f32>(0.16644880,0.32109205,0.17574891);}
 if tile==35{return vec3<f32>(0.16569308,0.31986656,0.17501362);}
 if tile==36{return vec3<f32>(0.16309913,0.31642157,0.17270561);}
 if tile==37{return vec3<f32>(0.96225490,0.52310730,0.14091095);}
 if tile==38{return vec3<f32>(0.97115332,0.56880447,0.17783905);}
 if tile==39{return vec3<f32>(0.98772467,0.65448666,0.23805147);}
 if tile==40{return vec3<f32>(0.94377723,0.45564406,0.10972903);}
 if tile==41{return vec3<f32>(0.97953431,0.61130174,0.20058551);}
 if tile==42{return vec3<f32>(0.97046569,0.56032135,0.16506672);}
 if tile==43{return vec3<f32>(0.98204657,0.62770289,0.22244009);}
 if tile==44{return vec3<f32>(0.97249455,0.57286901,0.17500681);}
 if tile==45{return vec3<f32>(0.95632489,0.50240332,0.13588644);}
 if tile==46{return vec3<f32>(0.03848039,0.04840686,0.08996460);}
 if tile==47{return vec3<f32>(0.33696215,0.34112881,0.35018382);}
return vec3<f32>(0.3);}
fn woodlandSample(tile:i32,uv:vec2<f32>,footprint:f32)->vec3<f32>{
 let p=fract(uv)*24.0-vec2<f32>(0.5);let i=vec2<i32>(floor(p));let f=fract(p);
 let color=mix(mix(woodlandFetch(tile,i).rgb,woodlandFetch(tile,i+vec2<i32>(1,0)).rgb,f.x),mix(woodlandFetch(tile,i+vec2<i32>(0,1)).rgb,woodlandFetch(tile,i+vec2<i32>(1,1)).rgb,f.x),f.y);
 return mix(color,woodlandAverage(tile),smoothstep(1.0,8.0,footprint*24.0));
}
// END GENERATED WOODLAND MATERIALS
@fragment fn fs_main(i:Output,@builtin(front_facing) front:bool)->@location(0) vec4<f32> {
 let dx=dpdx(i.worldPosition);let dy=dpdy(i.worldPosition);let ux=dpdx(i.uv);let uy=dpdy(i.uv);let footprint=max(length(ux),length(uy));
 if i.properties.x>0.5&&i.properties.x<5.5&&silhouette(i.uv,i.properties.x)<0.0{discard;}
 var n=normalize(i.normal);var base=i.color;
 if i.properties.x<0.5{let ridge=sin(i.uv.x*(i.ridges-floor(i.ridges/1000.0)*1000.0)*6.2831853+sin(i.uv.y*5.0)*0.7);let tangent=dx*uy.y-dy*ux.y;if dot(tangent,tangent)>0.0000001{n=normalize(n+normalize(tangent)*ridge*0.28);}base=woodlandSample(clamp(i32(floor(i.ridges/1000.0)),8,12),i.uv,footprint)*(0.86+0.14*ridge);}
 var transmission=i.properties.y;
 if i.properties.x>0.5&&i.properties.x<5.5&&!front{n=-n;}
 if i.properties.x>5.5 {
  let surface=treeSurface(i.uv,i.ridges,i.properties.y);if surface.a<0.1{discard;}
  let det=ux.x*uy.y-ux.y*uy.x;
  if abs(det)>0.0000000001{let right=normalize((dx*uy.y-dy*ux.y)/det);let up=normalize((-dx*uy.x+dy*ux.x)/det);let local=surface.rgb*2.0-vec3<f32>(1.0);n=normalize(right*local.x+up*local.y+n*local.z);}
  if surface.a>0.75{base=treeWood(i.ridges);transmission=0.0;}else{transmission=0.35;}
 }
 let light=normalize(scene.sunDirection.xyz);let view=normalize(-i.worldPosition);let vis=visibility(i.shadowPosition,n,light);
 let direct=max(dot(n,light),0.0);let back=max(dot(-n,light),0.0);let forward=pow(max(dot(-view,light),0.0),3.0);
 let transmitted=transmission*back*(0.45+0.55*forward);let exponent=mix(96.0,3.0,clamp(i.properties.z,0.0,1.0));
 let halfVector=light+view;var specular=0.0;if dot(halfVector,halfVector)>0.000001{specular=pow(max(dot(n,normalize(halfVector)),0.0),exponent)*0.035;}
 return vec4<f32>(base*(scene.ambientColor.xyz*i.properties.w+scene.sunColor.xyz*vis*(direct+transmitted))+scene.sunColor.xyz*specular*vis,1.0);
}
@fragment fn fs_shadow(i:Output){if i.properties.x>5.5{if treeSurface(i.uv,i.ridges,i.properties.y).a<0.1{discard;}return;}if i.properties.x>0.5&&silhouette(i.uv,i.properties.x)<0.0{discard;}}
