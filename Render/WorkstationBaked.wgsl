// AUTHOR POLICY: image composition ONLY. Offline artwork owns gem lighting.
// Do not add gemTrace, Fresnel, shadow/visibility, reflection/refraction or
// optical refresh work to either entry. Dynamic text and displays are unlit.
struct BakedImages {rows:array<vec4<f32>>};
struct BakedSprites {rows:array<vec4<f32>>};
@group(0) @binding(2) var<storage,read> bakedImages:BakedImages;
@group(0) @binding(3) var<storage,read> bakedSprites:BakedSprites;
struct BubbleImages {pixels:array<u32>};
@group(0) @binding(4) var<storage,read> bubbleImages:BubbleImages;
struct BakedVertex {@builtin(position) position:vec4<f32>,@location(0) point:vec2<f32>,@location(1) @interpolate(flat) sprite:u32};
struct BakedFragment {@location(0) color:vec4<f32>,@builtin(frag_depth) depth:f32};
// Perspective placement changes image coordinates only; lighting stays baked.
fn computerPosition(p:vec2<f32>)->vec4<f32>{
 let a=bakedSprites.rows[65536u];if(a.w<0.5){return vec4<f32>(p,0.49,1.0);}
 let b=bakedSprites.rows[65537u];let c=bakedSprites.rows[65538u];let q=vec3<f32>(p,1.0);let w=dot(c.xyz,q);
 return vec4<f32>(dot(a.xyz,q),dot(b.xyz,q),0.49*w,w);
}
@vertex fn vs_baked_overlay(@builtin(vertex_index) index:u32)->VertexOutput{
 var o:VertexOutput;let p=appearance.vertices[index].position;o.screen=p.xy/p.w;
 o.position=computerPosition(o.screen);o.cameraData=appearance.vertices[index].data;return o;
}
@vertex fn vs_baked(@builtin(vertex_index) vertex:u32,@builtin(instance_index) instance:u32)->BakedVertex{
 let corners=array<vec2<f32>,6>(vec2<f32>(-1.0,-1.0),vec2<f32>(1.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(-1.0,-1.0),vec2<f32>(1.0,1.0),vec2<f32>(-1.0,1.0));
 let rect=bakedSprites.rows[instance*8u];let p=rect.xy+corners[vertex]*rect.zw;
 var output:BakedVertex;output.position=computerPosition(vec2<f32>(p.x/gemRow(2).w,p.y/gemRow(3).w));output.point=p;output.sprite=instance;return output;
}
fn bakedDepth(z:f32)->f32{return clamp(0.49-z*0.01,0.0,0.4999);}
// The baked front surface is the protective window (material 131), not
// the spectrum behind it. Project the authored two 5mm lanes through that
// fixed artwork, then reuse the live palette/level calculation. No optics.
fn bakedMeterColor(point:vec3<f32>,spritePanel:i32,frontMaterial:i32)->vec4<f32>{
 // The disk coverage/material mask owns the aperture, including its edge pixels.
 // Never paint the display over the surrounding casing or its bevels.
 if(frontMaterial!=131){return vec4<f32>(0.0);}
 if(spritePanel<800000||spritePanel>=853760){return vec4<f32>(0.0);}
 let instance=(spritePanel-800000)/256;
 if(instance%3!=2){return vec4<f32>(0.0);}
 let local=(point-gemMixerTranslation(instance))/gemMixerScale(instance);
 if(abs(local.x)>0.005||abs(local.y)>0.078){return vec4<f32>(0.0);}
 let roles=gemRow(43157);let material=select(roles.x,roles.z,local.x>=0.0);
 return gemMixerMeterColor(point,800000+instance*256+i32(material));
}
fn bakedPixel(offset:u32,edge:u32,p:vec2<i32>)->vec4<f32>{let xy=clamp(p,vec2<i32>(0),vec2<i32>(i32(edge)-1));return bakedImages.rows[offset+u32(xy.y)*edge*2u+u32(xy.x)];}
// Offline reflected radiance was graded BEFORE alpha unpremultiplication.
// Composite E + (1-alpha)*background; preserve bright glints through clear film.
fn bubblePixel(address:u32,width:u32,height:u32,p:vec2<i32>)->vec4<f32>{
 let xy=clamp(p,vec2<i32>(0),vec2<i32>(i32(width)-1,i32(height)-1));
 let word=bubbleImages.pixels[address+u32(xy.y)*width+u32(xy.x)];
 let rgba=vec4<f32>(f32(word&255u),f32((word>>8u)&255u),f32((word>>16u)&255u),f32(word>>24u))/255.0;
 return rgba;
}
fn bubbleSample(address:u32,width:u32,height:u32,uv:vec2<f32>)->vec4<f32>{
 let p=uv*vec2<f32>(f32(width),f32(height))-0.5;let low=vec2<i32>(floor(p));let f=fract(p);
 return mix(mix(bubblePixel(address,width,height,low),bubblePixel(address,width,height,low+vec2<i32>(1,0)),f.x),
            mix(bubblePixel(address,width,height,low+vec2<i32>(0,1)),bubblePixel(address,width,height,low+vec2<i32>(1,1)),f.x),f.y);
}
@fragment fn fs_baked(input:BakedVertex)->BakedFragment{
 if(bakedSprites.rows[65536u].w>0.5 && (abs(input.point.x)>gemRow(2).w || abs(input.point.y)>gemRow(3).w)){discard;}
 let at=input.sprite*8u;let spec=bakedSprites.rows[at+1u];let clip=bakedSprites.rows[at+2u];let mode=bakedSprites.rows[at+3u];let placement=bakedSprites.rows[at+5u];
 if(any(input.point<clip.xy)||any(input.point>clip.zw)){discard;}
 var output:BakedFragment;
 if(mode.x==5.0){
  let em=(input.point-placement.xy)/spec.y;let footprint=2.0*gemRow(3).w/(gemRow(8).y*spec.y);
  // One front density layer is the composited lettering coverage. The old
  // three shifted layers represented depth inside refracting glass; projecting
  // all of them directly makes three displaced copies of every word. Never
  // restore their screen-space sum or add live optics to recover that depth.
  let density=gemEngravedDensity(em,i32(spec.z),vec2<f32>(0.0),footprint,0);
  var coverage=density;
  var ink=gemDisplay(gemRow(214).xyz*2.0);
  if(CARDINAL_2D){ink=cardinalRow(input.sprite,0u).xyz;}
  if(mode.y>0.5&&!CARDINAL_2D){
   // Thicken only the compact/right labels by a fraction of a display pixel.
   // Keep the authored fracture density, font size and advances; no live optics.
   let stroke=max(0.018,footprint*0.32);
   coverage=max(coverage,gemEngravedDensity(em+vec2<f32>(stroke,0.0),i32(spec.z),vec2<f32>(0.0),footprint,0));
   coverage=max(coverage,gemEngravedDensity(em-vec2<f32>(stroke,0.0),i32(spec.z),vec2<f32>(0.0),footprint,0));
   coverage=max(coverage,gemEngravedDensity(em+vec2<f32>(0.0,stroke),i32(spec.z),vec2<f32>(0.0),footprint,0));
   coverage=max(coverage,gemEngravedDensity(em-vec2<f32>(0.0,stroke),i32(spec.z),vec2<f32>(0.0),footprint,0));
   ink=vec3<f32>(1.0,0.96,0.86);
  }
  let opacity=select(1.0,cardinalRow(input.sprite,0u).w,CARDINAL_2D);
  let alpha=clamp(coverage*select(8.0,16.0,mode.y>0.5),0.0,1.0)*opacity;if(alpha<0.003){discard;}
  output.color=vec4<f32>(ink*alpha,alpha);output.depth=bakedDepth(spec.x);return output;
 }
 if(CARDINAL_2D){return cardinalFragment(input);}
 if(mode.x==7.0){
  let bounds=bakedSprites.rows[at+4u];let scale=placement.w/0.018;
  // Bend the persistent baked film by a cheap coordinate warp; no live optics.
  let t=clamp((input.point.x-placement.x)/placement.z,0.0,1.0);
  let curve=gemRow(i32(mode.y));
  let offset=curve.x+t*(curve.y+t*(curve.z+t*curve.w));
  let p=(input.point-placement.xy-vec2<f32>(0.0,offset))/scale;let length=placement.z/scale;
  let cap=min(length*0.46,0.018*0.28);let sourceCap=min(bounds.x*0.46,0.018*0.28);
  var x=sourceCap+(p.x-cap)*(bounds.x-2.0*sourceCap)/max(length-2.0*cap,0.000000001);
  if(p.x<cap){x=p.x*sourceCap/max(cap,0.000000001);}
  else if(p.x>length-cap){x=bounds.x-(length-p.x)*sourceCap/max(cap,0.000000001);}
  let uv=vec2<f32>((x+bounds.y)/(bounds.x+2.0*bounds.y),1.0-(p.y-bounds.z)/(bounds.w-bounds.z));
  if(any(uv<vec2<f32>(0.0))||any(uv>vec2<f32>(1.0))){discard;}
  let images=bakedSprites.rows[at+6u];let timing=bakedSprites.rows[at+7u];let width=u32(timing.z);let height=u32(timing.w);
  let a=mix(bubbleSample(u32(images.x),width,height,uv),bubbleSample(u32(images.y),width,height,uv),timing.x);
  var color=a;
  if(timing.y>0.0){let b=mix(bubbleSample(u32(images.z),width,height,uv),bubbleSample(u32(images.w),width,height,uv),timing.x);color=mix(a,b,timing.y);}
  if(max(max(color.x,color.y),max(color.z,color.w))<=0.001){discard;}
  var rgb=color.xyz;
  if(mode.z>0.5){rgb=mix(rgb,vec3<f32>(0.45,0.6,1.0)*color.w,0.18);}
  // AgX was applied OFFLINE by Blender. No gemDisplay/optical evaluation here.
  output.color=vec4<f32>(rgb,color.w);output.depth=bakedDepth(spec.x);return output;
 }
 let bounds=bakedSprites.rows[at+4u];let local=(input.point-placement.xy)/placement.zw;
 var uv=(local-bounds.xy)/(bounds.zw-bounds.xy);
 // Fit the casing to the unchanged 156 mm LED strip, with 2 mm end rims.
 // This samples persistent artwork only; no lighting or asset refresh.
 if(mode.x==8.0||(mode.x==3.0&&mode.y>=800000.0&&mode.y<853760.0)){
  let instance=(i32(mode.y)-800000)/256;
  if(mode.x==8.0||instance%3==2){
   let y=abs(local.y);
   let sourceY=select(y,0.078+(y-0.078)*6.0,y>0.078);
   uv.y=(sign(local.y)*sourceY-bounds.y)/(bounds.w-bounds.y);
  }
 }
 let edge=u32(spec.w);let offset=u32(spec.z);
 // Keep panel bevels at their original thickness during layout resizing.
 // Artwork is sampled, never relit, even after movement has stopped.
 if(mode.x==0.0||mode.x==4.0){
  let size=(bounds.zw-bounds.xy);let oldBand=min(size*0.2,vec2<f32>(min(size.x,size.y)*0.2));
  let worldSize=size*placement.zw;let newBand=min(oldBand*min(placement.z,placement.w),worldSize*0.49);
  let center=placement.xy+(bounds.xy+bounds.zw)*0.5*placement.zw;let distance=abs(input.point-center);let inner=max(worldSize*0.5-newBand,vec2<f32>(0.000001));
  let originalInner=size*0.5-oldBand;
  let mapped=select(distance*originalInner/inner,originalInner+(distance-inner)*oldBand/max(newBand,vec2<f32>(0.000001)),distance>inner);
  uv=(sign(input.point-center)*mapped+size*0.5)/size;
 }
 if(any(uv<vec2<f32>(0.0))||any(uv>vec2<f32>(1.0))){discard;}
 let pixel=uv*f32(edge)-0.5;let nearest=vec2<i32>(floor(pixel+0.5));let auxiliary=bakedPixel(offset+edge,edge,nearest);
 if(auxiliary.w<0.5){discard;}
 let low=vec2<i32>(floor(pixel));let fraction=fract(pixel);
 var rgb=vec3<f32>(0.0);var sum=0.0;
 for(var y:i32=0;y<2;y++){for(var x:i32=0;x<2;x++){
  let texel=low+vec2<i32>(x,y);let c=bakedPixel(offset,edge,texel);let weight=select(1.0-fraction.x,fraction.x,x==1)*select(1.0-fraction.y,fraction.y,y==1)*c.w;rgb+=c.xyz*weight;sum+=weight;
 }}
 if(sum<=0.0){discard;}rgb/=sum;
 let z=spec.x+auxiliary.x*spec.y;let panel=i32(mode.y)+i32(auxiliary.y+0.5);let point=vec3<f32>(input.point,z);
 if(mode.x==2.0){let wave=gemTimelineWaveform(point,vec3<f32>(0.0,0.0,auxiliary.z),panel);if(wave.w>0.5){rgb=wave.xyz;}}
 // Cardinal meter colors are display colors: preserve the six-stop ramp and
 // 25% inactive fill without exposure, emission gain or AgX desaturation.
 var directMeter=false;
 if(mode.x==3.0){let meter=bakedMeterColor(point,i32(mode.y),i32(auxiliary.y+0.5));if(meter.w>0.5){rgb=meter.xyz;directMeter=true;}}
 // Lane meter copies share the mixer's quantized six-stop display ramp.
 if(mode.x==8.0&&i32(auxiliary.y+0.5)==131){
  let meterPose=bakedSprites.rows[at+5u];
  let localPoint=(input.point-meterPose.xy)/meterPose.zw;
  let y=clamp((localPoint.y+0.078)/0.156,0.0,1.0);
  let t=(min(floor(y*60.0),59.0)+0.5)/60.0*5.0;
  let band=min(i32(floor(t)),4);let blend=t-f32(band);
  var a=vec3<f32>(0.0,0.0,1.0);var b=vec3<f32>(0.0,1.0,1.0);
  if(band==1){a=vec3<f32>(0.0,1.0,1.0);b=vec3<f32>(0.0,1.0,0.0);}
  if(band==2){a=vec3<f32>(0.0,1.0,0.0);b=vec3<f32>(1.0,1.0,0.0);}
  if(band==3){a=vec3<f32>(1.0,1.0,0.0);b=vec3<f32>(1.0,0.6509803921568628,0.0);}
  if(band==4){a=vec3<f32>(1.0,0.6509803921568628,0.0);b=vec3<f32>(1.0,0.0,0.0);}
  let fill=select(mode.z,mode.w,localPoint.x>=0.0);
  rgb=mix(a,b,blend)*select(0.25,1.0,y<fill);directMeter=true;
 }
 // Fixed control icons are baked into their individual disk images.
 if(mode.x==3.0){let engraving=gemMixerEngraving(vec3<f32>(input.point,z+0.000001),vec3<f32>(0.0,0.0,-1.0),panel,10.0);rgb=engraving.xyz+engraving.w*rgb;}
 if(mode.x==1.0&&mode.y<1028.0){rgb*=1.0-0.14*mode.z;}
 // Edison outlet state is a cheap tint over persistent baked artwork.
 if(mode.x==1.0&&mode.y>=3332.0&&mode.y<7428.0){
  let flags=u32(mode.z+0.5);
  if((flags&4u)!=0u){rgb*=0.35;}
  if((flags&2u)!=0u){rgb=mix(rgb,vec3<f32>(0.08,0.3,0.9),0.35);}
 }
 if(mode.x==6.0){rgb*=bakedSprites.rows[at+6u].xyz;}
 var displayColor=rgb;if(!directMeter){displayColor=gemDisplay(rgb);}
 // Paused browser: a display tint over fixed artwork, never an optical bake.
 if(mode.x==0.0&&mode.y==2.0&&mode.z>0.5){let grey=dot(displayColor,vec3<f32>(0.2126,0.7152,0.0722));displayColor=vec3<f32>(0.12+0.48*grey);}
 // Selection is a display tint of persistent artwork, never live optics.
 // Preserve facet brightness and waveform detail, with Cardinal's blue palette.
 if((mode.x==2.0||mode.x==4.0)&&mode.z>0.5){
  displayColor=vec3<f32>(0.45,0.72,1.0)*max(displayColor.x,max(displayColor.y,displayColor.z));
 }
 output.color=vec4<f32>(displayColor,1.0);output.depth=bakedDepth(z);return output;
}
@fragment fn fs_baked_overlay(input:VertexOutput)->BakedFragment{
 // The drum workspace has no timeline, mixer, or opaque panel overlay.
 if(gemRow(4041216).w>0.5){discard;}
 let origin=gemRow(1).xyz+gemRow(2).xyz*(input.screen.x*gemRow(2).w)+gemRow(3).xyz*(input.screen.y*gemRow(3).w);let direction=vec3<f32>(0.0,0.0,-1.0);
 if(gemPianoActive()){
  let piano=gemPianoHit(origin,direction,1e20);var color=gemDisplay(gemRow(6).xyz*gemRow(5).xyz);if(CARDINAL_2D){color=bakedSprites.rows[CARDINAL_GLOBAL_BASE+1u].xyz;}var depth=0.4999;var alpha=1.0;
  if(piano.panel>=950000){color=gemDisplay(gemRow(4041232+(piano.panel-950000)*3+2).xyz);depth=bakedDepth((origin+direction*piano.distance).z);}
  else if(gemRow(9).z>0.5&&gemPianoField(input.screen)){alpha=0.0;}
  let mark=gemTimelinePlayhead(origin,direction,1e20);if(mark.panel>=0){color=gemDisplay(select(vec3<f32>(0.75,0.5,0.08),vec3<f32>(0.08,0.35,0.85),mark.panel==719998));depth=bakedDepth((origin+direction*mark.distance).z);alpha=1.0;}
  var result:BakedFragment;result.color=vec4<f32>(gemPianoOverlay(color,input.screen)*alpha,alpha);result.depth=depth;return result;
 }
 var hit=gemBottomSurfaceHit(origin,direction,1e20);var color=vec3<f32>(0.0);var opacity=1.0;
 if(hit.panel>=0){color=gemBottomSurfaceColor(origin+direction*hit.distance);}
 let mark=gemTimelinePlayhead(origin,direction,hit.distance);if(mark.panel>=0){hit=mark;color=select(vec3<f32>(0.75,0.5,0.08),vec3<f32>(0.08,0.35,0.85),mark.panel==719998);}
 let drop=gemPluginDropMarker(origin,direction,hit.distance);if(drop.panel>=0){hit=drop;color=vec3<f32>(0.1,0.55,0.95);}
 if(hit.panel<0){discard;}
 var output:BakedFragment;output.color=vec4<f32>(gemDisplay(color)*opacity,opacity);output.depth=bakedDepth((origin+direction*hit.distance).z+0.00005);return output;
}

// Reuse the authored Plain studio color behind transparent artwork. Physical
// computer screens retain their dark backing instead of showing the theatre.
@fragment fn fs_baked_backdrop(input:VertexOutput)->BakedFragment{
 var output:BakedFragment;output.color=vec4<f32>(gemDisplay(gemRow(6).xyz*gemRow(5).xyz),1.0);
 if(gemRow(9).x<0.0){output.color=vec4<f32>(0.045,0.05,0.06,1.0);}
 if(CARDINAL_2D){output.color=bakedSprites.rows[CARDINAL_GLOBAL_BASE];
 let grain=bakedSprites.rows[CARDINAL_GLOBAL_BASE+22u].x;
 if(grain>0.0001){output.color=vec4<f32>(clamp(output.color.xyz*(1.0+cardinalGrain(input.screen*gemRow(8).xy,true)*grain*0.56),vec3<f32>(0.0),vec3<f32>(1.0)),output.color.w);}
 output.color=vec4<f32>(output.color.xyz*output.color.w,output.color.w);}output.depth=0.4999;return output;
}
