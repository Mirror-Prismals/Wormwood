// Cardinal's 2D presentation is an explicit alternative. The 3D gem ruby
// remains Wormwood's main/default theme. No optical work runs in these entries.
const CARDINAL_2D:bool=false;
const CARDINAL_STYLE_BASE:u32=65540u;
const CARDINAL_GLOBAL_BASE:u32=131076u;
fn cardinalRow(sprite:u32,slot:u32)->vec4<f32>{return bakedSprites.rows[CARDINAL_STYLE_BASE+sprite*8u+slot];}
fn cardinalAnchor(index:f32)->vec2<f32>{
 if(index<0.5){return vec2<f32>(0.0);}if(index<1.5){return vec2<f32>(1.0,0.0);}
 if(index<2.5){return vec2<f32>(0.0,1.0);}if(index<3.5){return vec2<f32>(1.0);}return vec2<f32>(0.5);
}
// Exact anchor/warp/smoothstep equations from Cardinal Host.h's
// DawThemeGradientLogic::Sample; the old renderer sampled these on its mesh.
fn cardinalGradient(uv:vec2<f32>,a:vec3<f32>,b:vec3<f32>,g:vec4<f32>)->vec3<f32>{
 let warp=clamp(g.w,0.0,1.0);let u=uv.x;let v=uv.y;
 let p=uv+warp*0.105*vec2<f32>(sin(v*6.2831853+u*1.7),sin(u*6.2831853-v*1.3+0.8));
 let da=distance(p,cardinalAnchor(g.y));let db=distance(p,cardinalAnchor(g.z));
 var blend=da/max(0.0001,da+db)+warp*0.065*sin((u+v)*7.0+sin(u*4.0)*0.8);
 blend=clamp(blend,0.0,1.0);blend=blend*blend*(3.0-2.0*blend);return clamp(mix(a,b,blend),vec3<f32>(0.0),vec3<f32>(1.0));
}
fn cardinalMeter(y:f32,level:f32,family:u32)->vec3<f32>{
 let t=(min(floor(clamp(y,0.0,1.0)*60.0),59.0)+0.5)/60.0*5.0;
 let index=min(u32(floor(t)),4u);let a=bakedSprites.rows[CARDINAL_GLOBAL_BASE+4u+min(family,2u)*6u+index].xyz;
 let b=bakedSprites.rows[CARDINAL_GLOBAL_BASE+5u+min(family,2u)*6u+index].xyz;
 return mix(a,b,fract(t))*select(0.25,1.0,y<level);
}
fn cardinalIcon(uv:vec2<f32>,icon:f32)->bool{
 let p=(uv-0.5)*2.0;
 if(icon==0.0){return (p.x<0.0&&p.x > -0.55&&abs(p.y)<(p.x+0.55))||(p.x>=0.0&&p.x<0.55&&abs(p.y)<p.x);}
 if(icon==1.0){return max(abs(p.x),abs(p.y))<0.43;}
 if(icon==2.0){return dot(p,p)<0.22;}
 if(icon==3.0){return p.x > -0.35&&p.x<0.55&&abs(p.y)<(0.55-p.x)*0.65;}return false;
}
fn cardinalFragment(input:BakedVertex)->BakedFragment{
 let at=input.sprite*8u;let rect=bakedSprites.rows[at];let mode=bakedSprites.rows[at+3u];
 let color=cardinalRow(input.sprite,0u);let g=cardinalRow(input.sprite,3u);let effects=cardinalRow(input.sprite,4u);let info=cardinalRow(input.sprite,6u);
 let uv=vec2<f32>((input.point.x-(rect.x-rect.z))/(2.0*rect.z),1.0-(input.point.y-(rect.y-rect.w))/(2.0*rect.w));
 var rgb=color.xyz;
 let crystal=cardinalRow(input.sprite,7u);
 if(crystal.w>0.5){
  let sample=uv*crystal.y-0.5;let xy=vec2<i32>(floor(sample));let f=fract(sample);
  let a=cardinalCrystalPixel(crystal,xy);let b=cardinalCrystalPixel(crystal,xy+vec2<i32>(1,0));
  let c=cardinalCrystalPixel(crystal,xy+vec2<i32>(0,1));let d=cardinalCrystalPixel(crystal,xy+vec2<i32>(1,1));
  let pixel=mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
  if(pixel.w<0.5){discard;}rgb=pixel.xyz;
 }
 if(crystal.w<0.5&&g.x>0.5){rgb=cardinalGradient(uv,cardinalRow(input.sprite,1u).xyz,cardinalRow(input.sprite,2u).xyz,g);}
 // Cardinal original panels/buttons use a bright top (+.10) and dark left
 // (-.03) face. Keep logical-pixel bevel widths under resizing and high DPI.
 let pixel=2.0*gemRow(3).w/max(gemRow(8).y,1.0);
 let band=min(vec2<f32>(0.22),vec2<f32>(5.0*pixel)/(2.0*rect.zw));
 if(crystal.w<0.5&&mode.x!=7.0&&mode.x!=6.0){if(uv.y<band.y){rgb=clamp(rgb+0.10,vec3<f32>(0.0),vec3<f32>(1.0));}else if(uv.x<band.x){rgb=clamp(rgb-0.03,vec3<f32>(0.0),vec3<f32>(1.0));}}
 if(info.w>0.5){rgb=cardinalJelly(uv,2.0*rect.zw/pixel,color.xyz,mode.z);}
 if(mode.x==1.0){rgb*=1.0-0.14*mode.z;}
 if(info.z>0.5){
  var level=select(mode.z,mode.w,uv.x>=0.5);var family=0u;
  if(mode.x==3.0){let instance=(i32(mode.y)-800000)/256;let record=gemMixerRecord(instance);let levels=gemRow(record+3);level=select(levels.z,levels.w,uv.x>=0.5);family=u32(clamp(gemRow(record+5).z,0.0,2.0));}
  rgb=cardinalMeter(1.0-uv.y,level,family);
 }
 if(mode.x==2.0){let wave=gemTimelineWaveform(vec3<f32>(input.point,bakedSprites.rows[at+1u].x),vec3<f32>(0.0,0.0,1.0),i32(mode.y));if(wave.w>0.5){rgb=wave.xyz;}}
 if(mode.x==6.0){rgb*=bakedSprites.rows[at+6u].xyz;}
 if((mode.x==2.0||mode.x==4.0||mode.x==7.0)&&mode.z>0.5){rgb=mix(rgb,vec3<f32>(0.45,0.72,1.0),0.35);}
 if(cardinalIcon(uv,info.x)){rgb=cardinalRow(input.sprite,5u).xyz;}
 if(effects.y>0.0001&&crystal.w<0.5){
  let p=uv*(2.0*rect.zw/pixel);
  let noise=cardinalGrain(p,false);
  rgb=clamp(rgb*(1.0+noise*effects.y*0.56),vec3<f32>(0.0),vec3<f32>(1.0));
 }
 // Emission is a bounded display-color adjustment, never a light or shadow.
 rgb=clamp(rgb*(1.0+effects.x),vec3<f32>(0.0),vec3<f32>(1.0));
 if(mode.x==0.0&&mode.y==2.0&&mode.z>0.5){rgb=vec3<f32>(0.12+0.48*dot(rgb,vec3<f32>(0.2126,0.7152,0.0722)));}
 var output:BakedFragment;output.color=vec4<f32>(rgb*color.w,color.w);output.depth=bakedDepth(bakedSprites.rows[at+1u].x);return output;
}

// Fixed, unlit candy-shell color equations ported from Cardinal UiJellyNote.h.
// This is the original 2D paint, not refraction or a runtime optical resolve.
fn cardinalJelly(uv:vec2<f32>,size:vec2<f32>,tint:vec3<f32>,pressed:f32)->vec3<f32>{
 let w=size.x;let h=size.y;let v=uv.y;let radius=min(w*0.46,h*0.28);
 let d=max(0.0,radius-min(v*h,h-v*h));let inset=radius-sqrt(max(0.0,radius*radius-d*d));
 let px=uv.x*w;if(px<inset||px>w-inset){discard;}
 let side=min(px,w-px);let end=1.0-smoothstep(0.0,radius*1.8,side);
 let edge=exp(-min(side,min(v*h,(1.0-v)*h))/1.2);
 let luminance=dot(tint,vec3<f32>(0.2126,0.7152,0.0722));
 let pigment=clamp(mix(vec3<f32>(luminance),tint,1.28),vec3<f32>(0.025),vec3<f32>(0.95));
 var color=pigment*(0.76+0.23*sin(v*3.14159265)-0.19*end-0.23*edge);
 let seam=0.43-0.13*end;let reflection=smoothstep(0.035,0.10,v)*(1.0-smoothstep(seam-0.025,seam+0.025,v));
 color=mix(color,vec3<f32>(0.97,0.985,1.0),reflection*(0.61-0.30*v)*(1.0-0.38*end)*select(1.0,0.72,pressed>0.5));
 let glint=exp(-pow((v-0.13)/0.048,2.0))*exp(-pow((px-radius*1.8)/max(8.0,min(w*0.30,55.0)),2.0));
 color=mix(color,vec3<f32>(1.0),glint*0.66);
 color+=pigment*exp(-pow((v-0.83)/0.12,2.0))*0.31*(1.0-0.65*end);
 let lip=exp(-pow((v-0.945)/0.022,2.0));return clamp(mix(color,mix(pigment,vec3<f32>(1.0),0.63),lip*0.7*(1.0-end)),vec3<f32>(0.0),vec3<f32>(1.0));
}

fn cardinalMirror(value:i32)->u32{let period=2046;let wrapped=((value%period)+period)%period;return u32(select(wrapped,period-wrapped,wrapped>=1024));}
fn cardinalGrain(p:vec2<f32>,background:bool)->f32{
 let x=cardinalMirror(i32(round(p.x)));let y=cardinalMirror(i32(round(p.y)));
 let word=bubbleImages.pixels[y*1024u+x];let shift=select(0u,8u,background);
 return f32((word>>shift)&255u)/255.0-0.5;
}

fn cardinalCrystalPixel(spec:vec4<f32>,p:vec2<i32>)->vec4<f32>{
 let xy=vec2<u32>(clamp(p,vec2<i32>(0),vec2<i32>(i32(spec.y)-1)));
 let word=bubbleImages.pixels[1048576u+u32(spec.x)+xy.y*u32(spec.z)+xy.x];
 return vec4<f32>(f32(word&255u),f32((word>>8u)&255u),f32((word>>16u)&255u),f32(word>>24u))/255.0;
}
