// Exterior studio lighting remains unchanged, but a ray leaving the meter
// window must reach the spectrum inside its own authored assembly first.
fn retainedMeterTerminal(origin:vec3<f32>,ray:vec3<f32>)->vec4<f32> {
 if(gemMixerHit(origin,ray,-1,1e20,true).panel>=0){return vec4<f32>(0.0);}
 return gemTerminal(origin,ray);
}
// Exact original linear radiance, retained in independent component groups.
const retainedMergeStatic:bool=false;
const retainedTransportButtons:bool=false;
struct RetainedMetadata { rows:array<vec4<f32>> };
@group(0) @binding(4) var retainedOut:texture_storage_2d_array<rgba32float,write>;
@group(0) @binding(5) var retainedIn:texture_2d_array<f32>;
@group(0) @binding(6) var<storage,read> retainedMeta:RetainedMetadata;
var<private> retainedActive:bool=false;
var<private> retainedPrimary:bool=false;
var<private> retainedGroup:i32=0;
var<private> retainedOrigin:vec3<f32>;
fn retainedControlGroup(instance:i32)->i32 {
 // I/O belongs to logical right. Keep metadata rows49..57 reserved for the
 // captured transport bounds/poses; the old65-entry CPU table stays bounded.
 if(instance>=65&&instance<128){return 4;}
 return i32(retainedMeta.rows[32u+u32(instance)/4u][u32(instance)%4u]);
}
fn gemNearest(origin:vec3<f32>,ray:vec3<f32>,medium:i32,farLimit:f32)->GemHit {
 let primary=retainedActive&&medium<0&&all(origin==retainedOrigin);
 retainedPrimary=primary;
 let hit=retainedOriginalNearest(origin,ray,medium,farLimit);
 retainedPrimary=false;
 return hit;
}
fn retainedRay(pixel:vec2<f32>,sample:i32)->vec3<f32> {
 let resolution=gemRow(8).xy;
 let screen=vec2<f32>((pixel.x+0.5)*2.0/resolution.x-1.0,1.0-(pixel.y+0.5)*2.0/resolution.y);
 let xi=fract(vec2<f32>(0.375,0.125)+vec2<f32>(0.5,0.75)*f32(sample));
 let p=screen+(xi-vec2<f32>(0.5))*2.0/resolution;
 return gemRow(1).xyz+gemRow(2).xyz*(p.x*gemRow(2).w)+gemRow(3).xyz*(p.y*gemRow(3).w);
}
@compute @workgroup_size(8,8,1)
fn cs_retained(@builtin(global_invocation_id) id:vec3<u32>) {
 let group=i32(id.z)/2;let sample=i32(id.z)%2;
 let captureMask=u32(retainedMeta.rows[1u].x);
 if(captureMask!=0u&&(captureMask&(1u<<u32(group)))==0u){return;}
 let rect=retainedMeta.rows[2u+u32(group)*4u];
 if(any(id.xy>=vec2<u32>(rect.zw))){return;}
 let origin=retainedRay(vec2<f32>(id.xy)+rect.xy,sample);
 let direction=gemSafeUnit(gemRow(4).xyz);
 let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sample));
 retainedActive=true;retainedGroup=select(group,6,retainedTransportButtons&&group==5);retainedOrigin=origin;
 var value=vec4<f32>(0.0,0.0,0.0,1e20);
 if(retainedMergeStatic&&group==0){
  // Groups 0 and 5 always have the same identity transform. Bake their
  // exact depth winner once; secondary rays retain the complete scene.
  retainedGroup=5;
  let h=gemNearest(origin,direction,-1,1e20);
  if(h.panel>=0&&h.distance<1e19){
   value=vec4<f32>(gemTrace(origin,direction,random),h.distance);
  }else{
   // At a depth tie the old group-0 background wins, before group 5.
   retainedGroup=0;
   value=vec4<f32>(gemTrace(origin,direction,random),1e19);
  }
 }else{
  let h=gemNearest(origin,direction,-1,1e20);
  if(group==0||h.panel>=0){value=vec4<f32>(gemTrace(origin,direction,random),select(h.distance,1e19,group==0));}
 }
 textureStore(retainedOut,vec2<i32>(id.xy),i32(id.z),value);
}
// Retain the captured RGB exactly. Orthographic -Z projection means an inward
// inward travel changes depth; Cardinal lateral travel remaps actual pose XY.
// Captured pose is authoritative, including a capture taken during a press.
fn retainedButtonSample(p:vec2<f32>,sample:i32,button:u32)->vec4<f32> {
 let rect=retainedMeta.rows[22u];
 let captured=retainedMeta.rows[53u+button];
 let current=gemRow(192+i32(button)*2);
 if(captured.w<=0.0||current.w<=0.0){return vec4<f32>(0.0,0.0,0.0,1e20);}
 let bounds=retainedMeta.rows[49u+button];
 var source=p;
 if(any(current.xy!=captured.xy)||current.w!=captured.w){
  let center=vec2<f32>((current.x/gemRow(2).w+1.0)*gemRow(8).x*0.5,(1.0-current.y/gemRow(3).w)*gemRow(8).y*0.5);
  source=(p+vec2<f32>(0.5)-center)*(captured.w/current.w)+bounds.xy-vec2<f32>(0.5);
 }
 if(any(abs(source-bounds.xy)>bounds.zw)){return vec4<f32>(0.0,0.0,0.0,1e20);}
 let xy=vec2<i32>(floor(source-rect.xy+vec2<f32>(0.5)));
 if(any(xy<vec2<i32>(0))||any(xy>=vec2<i32>(rect.zw))){return vec4<f32>(0.0,0.0,0.0,1e20);}
 var v=textureLoad(retainedIn,xy,10+sample,0);
 if(v.w>=1e19){return v;}
 // The identity branch preserves captured depth bits as well as all RGB bits.
 let captureOriginZ=retainedMeta.rows[57u].x;
 if(current.z!=captured.z||current.w!=captured.w||gemRow(1).z!=captureOriginZ){
  let localZ=(captureOriginZ-v.w-captured.z)/captured.w;
  v=vec4<f32>(v.xyz,gemRow(1).z-(current.z+localZ*current.w));
 }
 return v;
}
@fragment fn fs_retained(input:VertexOutput)->@location(0) vec4<f32> {
 if(input.cameraData.w>=99.5 && input.cameraData.w<=104.5){return theatreSample(input.cameraData);}if(input.cameraData.w>89.5){return vec4<f32>(input.cameraData.xyz,1.0);}
 var hasForeground=false;
 let p=floor(input.position.xy);var total=vec3<f32>(0.0);
 for(var sample:i32=0;sample<2;sample++) {
  var best=vec4<f32>(0.0,0.0,0.0,1e20);var winner:i32=6;
  if(retainedMergeStatic){
   best=textureLoad(retainedIn,vec2<i32>(p),sample,0);
   winner=select(5,0,best.w==1e19);
  }
  for(var group:i32=select(0,1,retainedMergeStatic);group<select(6,5,retainedMergeStatic&&!retainedTransportButtons);group++) {
   if(retainedTransportButtons&&group==5){
    for(var button:u32=0u;button<4u;button++){let v=retainedButtonSample(p,sample,button);if(v.w<best.w){best=v;winner=group;}}
    continue;
   }
   let at=2u+u32(group)*4u;let rect=retainedMeta.rows[at];
   let old=retainedMeta.rows[at+1u];let now=retainedMeta.rows[at+2u];
   let source=(p+vec2<f32>(0.5)-now.xy)*old.zw/max(now.zw,vec2<f32>(0.000001))+old.xy-vec2<f32>(0.5);
   let xy=vec2<i32>(floor(source-rect.xy+vec2<f32>(0.5)));
   if(any(xy<vec2<i32>(0))||any(xy>=vec2<i32>(rect.zw))){continue;}
   let v=textureLoad(retainedIn,xy,group*2+sample,0);
   // Preserve the old 0,1,2,3,4,5 order for exact depth ties.
   if(v.w<best.w||(retainedMergeStatic&&v.w==best.w&&group<winner)){best=v;winner=group;}
  }
  let origin=retainedRay(p,sample);let direction=gemSafeUnit(gemRow(4).xyz);
  // COMPOSITED_RULERS
  // Live scope/waveform/selection uses the same original surface shader.
  // It is excluded from the baked primary image, so moving its content cannot
  // leave a stale picture or trigger a full optical refresh.
  let surface=gemBottomSurfaceHit(origin,direction,best.w);
  if(surface.panel>=0){let point=origin+direction*surface.distance;best=vec4<f32>(gemOpaque(point,-direction,surface.normal,gemBottomSurfaceColor(point),0.35,false),surface.distance);}
  // The authored LEDs sit behind a refractive protective window. Reusing
  // its baked radiance freezes their levels: the first hit is glass, not an
  // LED material. Refresh only visible meter apertures with the same optical
  // integrator and sample sequence used for the retained image. Housings and
  // the rest of the workstation remain cached.
  if(gemRow(43152).w>0.5){
   for(var meter:i32=0;meter<70;meter++){
    if(f32(meter)>=gemRow(43152).x){break;}
    let instance=meter*3+2;let at=43160+meter*6;let box=gemRow(at+2);let clip=gemRow(at+4);
    if(any(abs(origin.xy-box.xy)>box.zw*0.5+vec2<f32>(0.000001))||any(origin.xy<clip.xy)||any(origin.xy>clip.zw)){continue;}
    if(!gemMixerActive(instance)){continue;}
    let hit=gemMixerTraceInstance(origin,direction,instance,-1,best.w+0.000001,false);
    if(hit.panel<0||hit.distance>best.w+0.000001){continue;}
    let random=fract(vec2<f32>(0.324717957,0.754877666)+vec2<f32>(0.618033989,0.414213562)*f32(sample));
    best=vec4<f32>(retainedMeterTrace(origin,direction,random),hit.distance);
    break;
   }
  }
  let mark=gemTimelinePlayhead(origin,direction,best.w);
  if(mark.panel>=0){best=vec4<f32>(gemOpaque(origin+direction*mark.distance,-direction,mark.normal,vec3<f32>(0.75,0.5,0.08),0.24,false),mark.distance);}
  hasForeground=hasForeground||best.w<1e19;
  total+=best.xyz;
 }
 if(gemRow(9).z>0.5&&!hasForeground){discard;}
 return vec4<f32>(gemDisplay(total*0.5),1.0);
}
