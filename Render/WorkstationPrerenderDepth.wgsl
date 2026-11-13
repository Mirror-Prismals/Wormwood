// Separate exact FP32 depth permits visibility selection before fetching radiance.
@group(0) @binding(7) var retainedDepthOut:texture_storage_2d_array<r32float,write>;
@group(0) @binding(8) var retainedDepthIn:texture_2d_array<f32>;
@fragment fn fs_retained(input:VertexOutput)->@location(0) vec4<f32> {
 if(input.cameraData.w>=99.5 && input.cameraData.w<=104.5){return theatreSample(input.cameraData);}
 if(input.cameraData.w>89.5){return vec4<f32>(input.cameraData.xyz,1.0);}
 let p=floor(input.position.xy);var total=vec3<f32>(0.0);
 // Mapping and bounds are sample independent. Keep the original operations.
 var coordinates:array<vec2<i32>,5>;var valid:u32=0u;
 for(var group:i32=1;group<5;group++) {
  let at=2u+u32(group)*4u;let rect=retainedMeta.rows[at];
  let old=retainedMeta.rows[at+1u];let now=retainedMeta.rows[at+2u];
  let source=(p+vec2<f32>(0.5)-now.xy)*old.zw/max(now.zw,vec2<f32>(0.000001))+old.xy-vec2<f32>(0.5);
  let xy=vec2<i32>(floor(source-rect.xy+vec2<f32>(0.5)));
  coordinates[group]=xy;
  if(all(xy>=vec2<i32>(0))&&all(xy<vec2<i32>(rect.zw))){valid|=1u<<u32(group);}
 }
 for(var sample:i32=0;sample<2;sample++) {
  var best:vec4<f32>;
  if(valid==0u){
   // Background-only pixels keep their original single RGBA load.
   best=textureLoad(retainedIn,vec2<i32>(p),sample,0);
  }else{
   var depth=textureLoad(retainedDepthIn,vec2<i32>(p),sample,0).x;
   var winner=select(5,0,depth==1e19);var layer=sample;var coord=vec2<i32>(p);
   for(var group:i32=1;group<5;group++) {
    if((valid&(1u<<u32(group)))==0u){continue;}
    let xy=coordinates[group];let d=textureLoad(retainedDepthIn,xy,group*2+sample,0).x;
    // Identical group order and strict/tie comparison to the original path.
    if(d<depth||(d==depth&&group<winner)){depth=d;winner=group;layer=group*2+sample;coord=xy;}
   }
   best=textureLoad(retainedIn,coord,layer,0);
  }
  let origin=retainedRay(p,sample);let direction=gemSafeUnit(gemRow(4).xyz);
  let mark=gemTimelinePlayhead(origin,direction,best.w);
  if(mark.panel>=0){best=vec4<f32>(gemOpaque(origin+direction*mark.distance,-direction,mark.normal,vec3<f32>(0.75,0.5,0.08),0.24,false),mark.distance);}
  total+=best.xyz;
 }
 return vec4<f32>(gemDisplay(total*0.5),1.0);
}
